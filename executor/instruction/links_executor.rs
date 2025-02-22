/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt, iter,
    ops::Bound,
    sync::Arc,
};

use answer::{variable_value::VariableValue, Thing, Type};
use compiler::{executable::match_::instructions::thing::LinksInstruction, ExecutorVariable};
use concept::{
    error::ConceptReadError,
    thing::{
        relation::{LinksIterator, Relation, RolePlayer},
        thing_manager::ThingManager,
    },
    type_::{object_type::ObjectType, relation_type::RelationType},
};
use itertools::{kmerge_by, Itertools, KMergeBy, MinMaxResult};
use primitive::Bounds;
use resource::constants::traversal::CONSTANT_CONCEPT_LIMIT;
use storage::snapshot::ReadableSnapshot;

use crate::{
    instruction::{
        iterator::{SortedTupleIterator, TupleIterator},
        tuple::{
            links_to_tuple_player_relation_role, links_to_tuple_relation_player_role,
            links_to_tuple_role_relation_player, LinksToTupleFn, TuplePositions,
        },
        Checker, FilterFn, FilterMapFn, TernaryIterateMode, VariableModes,
    },
    pipeline::stage::ExecutionContext,
    row::MaybeOwnedRow,
};

pub(crate) struct LinksExecutor {
    links: ir::pattern::constraint::Links<ExecutorVariable>,

    iterate_mode: TernaryIterateMode,
    variable_modes: VariableModes,

    tuple_positions: TuplePositions,

    relation_player_types: Arc<BTreeMap<Type, Vec<Type>>>, // vecs are in sorted order
    relation_type_range: Bounds<RelationType>,
    player_type_range: Bounds<ObjectType>,

    filter_fn: Arc<LinksFilterFn>,
    relation_cache: Option<Vec<Relation>>,

    checker: Checker<(Relation, RolePlayer, u64)>,
}

pub(super) type LinksTupleIterator<I> = iter::Map<iter::FilterMap<I, Box<LinksFilterMapFn>>, LinksToTupleFn>;

pub(crate) type LinksUnboundedSortedRelation = LinksTupleIterator<LinksIterator>;
pub(crate) type LinksUnboundedSortedPlayerSingle = LinksTupleIterator<LinksIterator>;
pub(crate) type LinksUnboundedSortedPlayerMerged = LinksTupleIterator<KMergeBy<LinksIterator, LinksOrderingFn>>;
pub(crate) type LinksBoundedRelationSortedPlayer = LinksTupleIterator<LinksIterator>;
pub(crate) type LinksBoundedRelationPlayer = LinksTupleIterator<LinksIterator>;

pub(super) type LinksFilterFn = FilterFn<(Relation, RolePlayer, u64)>;
pub(super) type LinksFilterMapFn = FilterMapFn<(Relation, RolePlayer, u64)>;

type LinksVariableValueExtractor = fn(&(Relation, RolePlayer, u64)) -> VariableValue<'static>;
pub(super) const EXTRACT_RELATION: LinksVariableValueExtractor =
    |&(rel, _, _)| VariableValue::Thing(Thing::Relation(rel));
pub(super) const EXTRACT_PLAYER: LinksVariableValueExtractor =
    |(_, rp, _)| VariableValue::Thing(Thing::from(rp.player()));
pub(super) const EXTRACT_ROLE: LinksVariableValueExtractor =
    |(_, rp, _)| VariableValue::Type(Type::RoleType(rp.role_type()));

pub(crate) type LinksOrderingFn = for<'a, 'b> fn(
    &'a Result<(Relation, RolePlayer, u64), Box<ConceptReadError>>,
    &'b Result<(Relation, RolePlayer, u64), Box<ConceptReadError>>,
) -> bool;

impl LinksExecutor {
    pub(crate) fn new(
        links: LinksInstruction<ExecutorVariable>,
        variable_modes: VariableModes,
        sort_by: ExecutorVariable,
        snapshot: &impl ReadableSnapshot,
        thing_manager: &ThingManager,
    ) -> Result<Self, Box<ConceptReadError>> {
        debug_assert!(!variable_modes.all_inputs());
        let relation_player_types = links.relation_to_player_types().clone();
        debug_assert!(!relation_player_types.is_empty());
        let player_types = links.player_types().clone();
        let player_role_types = links.relation_to_role_types().clone();
        let LinksInstruction { links, checks, .. } = links;
        let iterate_mode = TernaryIterateMode::new(links.relation(), links.player(), &variable_modes, sort_by);
        let filter_fn = create_links_filter_relations_players_roles(relation_player_types.clone(), player_role_types);

        let relation = links.relation().as_variable().unwrap();
        let player = links.player().as_variable().unwrap();
        let role_type = links.role_type().as_variable().unwrap();

        let output_tuple_positions = match iterate_mode {
            TernaryIterateMode::Unbound => TuplePositions::Triple([Some(relation), Some(player), Some(role_type)]),
            TernaryIterateMode::UnboundInverted => {
                TuplePositions::Triple([Some(player), Some(relation), Some(role_type)])
            }
            TernaryIterateMode::BoundFrom => TuplePositions::Triple([Some(player), Some(relation), Some(role_type)]),
            TernaryIterateMode::BoundFromBoundTo => {
                TuplePositions::Triple([Some(role_type), Some(relation), Some(player)])
            }
        };

        let checker = Checker::<(Relation, RolePlayer, _)>::new(
            checks,
            HashMap::from([(relation, EXTRACT_RELATION), (player, EXTRACT_PLAYER), (role_type, EXTRACT_ROLE)]),
        );

        let relation_type_range = (
            Bound::Included(relation_player_types.first_key_value().unwrap().0.as_relation_type()),
            Bound::Included(relation_player_types.last_key_value().unwrap().0.as_relation_type()),
        );
        let (min_player_type, max_player_type) = min_max_types(player_types.iter());
        let player_type_range =
            (Bound::Included(min_player_type.as_object_type()), Bound::Included(max_player_type.as_object_type()));
        let relation_cache = if iterate_mode == TernaryIterateMode::UnboundInverted {
            let mut cache = Vec::new();
            for type_ in relation_player_types.keys() {
                let instances: Vec<Relation> =
                    thing_manager.get_relations_in(snapshot, type_.as_relation_type()).try_collect()?;
                cache.extend(instances);
            }
            #[cfg(debug_assertions)]
            if cache.len() < CONSTANT_CONCEPT_LIMIT {
                eprintln!("DEBUG_ASSERT_FAILURE: cache.len() > CONSTANT_CONCEPT_LIMIT");
            }
            Some(cache)
        } else {
            None
        };

        Ok(Self {
            links,
            iterate_mode,
            variable_modes,
            tuple_positions: output_tuple_positions,
            relation_player_types,
            relation_type_range,
            player_type_range,
            filter_fn,
            relation_cache,
            checker,
        })
    }

    pub(crate) fn get_iterator(
        &self,
        context: &ExecutionContext<impl ReadableSnapshot + 'static>,
        row: MaybeOwnedRow<'_>,
    ) -> Result<TupleIterator, Box<ConceptReadError>> {
        let filter = self.filter_fn.clone();
        let check = self.checker.filter_for_row(context, &row);
        let filter_for_row: Box<LinksFilterMapFn> = Box::new(move |item| match filter(&item) {
            Ok(true) => match check(&item) {
                Ok(true) | Err(_) => Some(item),
                Ok(false) => None,
            },
            Ok(false) => None,
            Err(_) => Some(item),
        });

        let snapshot = &**context.snapshot();
        let thing_manager = context.thing_manager();

        match self.iterate_mode {
            TernaryIterateMode::Unbound => {
                // TODO: we could cache the range byte arrays computed inside the thing_manager, for this case
                let iterator = thing_manager.get_links_by_relation_type_range(snapshot, &self.relation_type_range);
                let as_tuples: LinksUnboundedSortedRelation =
                    iterator.filter_map(filter_for_row).map(links_to_tuple_relation_player_role as _);
                Ok(TupleIterator::LinksUnbounded(SortedTupleIterator::new(
                    as_tuples,
                    self.tuple_positions.clone(),
                    &self.variable_modes,
                )))
            }

            TernaryIterateMode::UnboundInverted => {
                debug_assert!(self.relation_cache.is_some());
                if let Some([relation]) = self.relation_cache.as_deref() {
                    // no heap allocs needed if there is only 1 iterator
                    let iterator = thing_manager.get_links_by_relation_and_player_type_range(
                        snapshot,
                        *relation,
                        // TODO: this should be just the types owned by the one instance's type in the cache!
                        &self.player_type_range,
                    );
                    let as_tuples: LinksUnboundedSortedPlayerSingle =
                        iterator.filter_map(filter_for_row).map(links_to_tuple_player_relation_role);
                    Ok(TupleIterator::LinksUnboundedInvertedSingle(SortedTupleIterator::new(
                        as_tuples,
                        self.tuple_positions.clone(),
                        &self.variable_modes,
                    )))
                } else {
                    // TODO: we could create a reusable space for these temporarily held iterators
                    //       so we don't have allocate again before the merging iterator
                    let relations = self.relation_cache.as_ref().unwrap().iter();
                    let iterators = relations
                        .map(|&relation| {
                            thing_manager.get_links_by_relation_and_player_type_range(
                                snapshot,
                                relation,
                                &self.player_type_range,
                            )
                        })
                        .collect_vec();

                    // note: this will always have to heap alloc, if we use don't have a re-usable/small-vec'ed priority queue somewhere
                    let merged: KMergeBy<LinksIterator, LinksOrderingFn> =
                        kmerge_by(iterators, compare_by_player_then_relation);
                    let as_tuples: LinksUnboundedSortedPlayerMerged =
                        merged.filter_map(filter_for_row).map(links_to_tuple_player_relation_role);
                    Ok(TupleIterator::LinksUnboundedInvertedMerged(SortedTupleIterator::new(
                        as_tuples,
                        self.tuple_positions.clone(),
                        &self.variable_modes,
                    )))
                }
            }

            TernaryIterateMode::BoundFrom => {
                let relation = self.links.relation().as_variable().unwrap().as_position().unwrap();
                debug_assert!(row.len() > relation.as_usize());
                let iterator = match row.get(relation) {
                    &VariableValue::Thing(Thing::Relation(relation)) => thing_manager
                        .get_links_by_relation_and_player_type_range(snapshot, relation, &self.player_type_range),
                    _ => unreachable!("Links relation must be a relation."),
                };
                let as_tuples: LinksBoundedRelationSortedPlayer =
                    iterator.filter_map(filter_for_row).map(links_to_tuple_player_relation_role);
                Ok(TupleIterator::LinksBoundedRelation(SortedTupleIterator::new(
                    as_tuples,
                    self.tuple_positions.clone(),
                    &self.variable_modes,
                )))
            }

            TernaryIterateMode::BoundFromBoundTo => {
                let relation = self.links.relation().as_variable().unwrap().as_position().unwrap();
                let player = self.links.player().as_variable().unwrap().as_position().unwrap();
                debug_assert!(row.len() > relation.as_usize());
                debug_assert!(row.len() > player.as_usize());
                let relation = row.get(relation).as_thing().as_relation();
                let player = row.get(player).as_thing().as_object();
                let iterator = thing_manager.get_links_by_relation_and_player(snapshot, relation, player);
                let as_tuples: LinksBoundedRelationSortedPlayer =
                    iterator.filter_map(filter_for_row).map(links_to_tuple_role_relation_player);
                Ok(TupleIterator::LinksBoundedRelationPlayer(SortedTupleIterator::new(
                    as_tuples,
                    self.tuple_positions.clone(),
                    &self.variable_modes,
                )))
            }
        }
    }
}

impl fmt::Display for LinksExecutor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "[{}], mode={}", &self.links, &self.iterate_mode)
    }
}

fn create_links_filter_relations_players_roles(
    relation_to_player: Arc<BTreeMap<Type, Vec<Type>>>,
    player_to_role: Arc<BTreeMap<Type, BTreeSet<Type>>>,
) -> Arc<LinksFilterFn> {
    Arc::new(move |result| {
        let (rel, rp) = match result {
            Ok((rel, rp, _)) => (rel, rp),
            Err(err) => return Err(err.clone()),
        };
        let Some(player_types) = relation_to_player.get(&Type::from(rel.type_())) else {
            return Ok(false);
        };
        let player_type = Type::from(rp.player().type_());
        let role_type = Type::from(rp.role_type());
        Ok(player_types.contains(&player_type)
            && player_to_role.get(&player_type).is_some_and(|role_types| role_types.contains(&role_type)))
    })
}

fn compare_by_player_then_relation(
    left: &Result<(Relation, RolePlayer, u64), Box<ConceptReadError>>,
    right: &Result<(Relation, RolePlayer, u64), Box<ConceptReadError>>,
) -> bool {
    if let (Ok((rel_1, rp_1, _)), Ok((rel_2, rp_2, _))) = (left, right) {
        (rp_1.player(), rel_1) < (rp_2.player(), rel_2)
    } else {
        false
    }
}

fn min_max_types<'a>(types: impl IntoIterator<Item = &'a Type>) -> (Type, Type) {
    match types.into_iter().minmax() {
        MinMaxResult::NoElements => unreachable!("Empty type iterator"),
        MinMaxResult::OneElement(item) => (*item, *item),
        MinMaxResult::MinMax(min, max) => (*min, *max),
    }
}
