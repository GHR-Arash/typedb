/*
 * Copyright (C) 2020 Grakn Labs
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 *
 */

package grakn.core.concept.schema.impl;

import grakn.core.common.exception.GraknException;
import grakn.core.concept.schema.Rule;
import grakn.core.concept.type.impl.RelationTypeImpl;
import grakn.core.concept.type.impl.RoleTypeImpl;
import grakn.core.concept.type.impl.TypeImpl;
import grakn.core.graph.GraphManager;
import grakn.core.graph.vertex.RuleVertex;
import grakn.core.graph.vertex.TypeVertex;
import grakn.core.pattern.Conjunction;
import grakn.core.pattern.constraint.Constraint;
import grakn.core.pattern.constraint.thing.RelationConstraint;
import grakn.core.pattern.constraint.type.LabelConstraint;
import grakn.core.pattern.variable.ThingVariable;
import grakn.core.pattern.variable.Variable;

import java.util.Collections;
import java.util.Set;
import java.util.stream.Stream;

import static grakn.core.common.exception.ErrorMessage.TypeRead.TYPE_NOT_FOUND;
import static grakn.core.graph.util.Encoding.Edge.Rule.CONCLUSION;
import static grakn.core.graph.util.Encoding.Edge.Rule.CONDITION_NEGATIVE;
import static grakn.core.graph.util.Encoding.Edge.Rule.CONDITION_POSITIVE;

public class RuleImpl implements Rule {

    private final GraphManager graphMgr;
    private final RuleVertex vertex;
    private Conjunction when;
    private Set<Variable> then;

    private RuleImpl(final GraphManager graphMgr, final RuleVertex vertex) {
        this.graphMgr = graphMgr;
        this.vertex = vertex;
    }

    private RuleImpl(final GraphManager graphMgr, final String label,
                     final graql.lang.pattern.Conjunction<? extends graql.lang.pattern.Pattern> when,
                     final graql.lang.pattern.variable.ThingVariable<?> then) {
        graql.lang.pattern.schema.Rule.validate(label, when, then);
        this.graphMgr = graphMgr;
        this.vertex = graphMgr.schema().create(label, when, then);
        putPositiveConditions();
        putNegativeConditions();
        putConclusions();
        validate();
    }

    public static RuleImpl of(final GraphManager graphMgr, final RuleVertex vertex) {
        return new RuleImpl(graphMgr, vertex);
    }

    public static RuleImpl of(final GraphManager graphMgr, final String label,
                              final graql.lang.pattern.Conjunction<? extends graql.lang.pattern.Pattern> when,
                              final graql.lang.pattern.variable.ThingVariable<?> then) {
        return new RuleImpl(graphMgr, label, when, then);
    }

    private void putPositiveConditions() {
        vertex.outs().delete(CONDITION_POSITIVE);
        when().constraints().stream()
                .filter(constraint -> constraint.isType() && constraint.asType().isLabel())
                .forEach(constraint -> {
                    LabelConstraint label = constraint.asType().asLabel();
                    if (label.scope().isPresent()) {
                        TypeVertex relation = graphMgr.schema().getType(label.scope().get());
                        TypeVertex role = graphMgr.schema().getType(label.label(), label.scope().get());
                        if (role == null) throw GraknException.of(TYPE_NOT_FOUND.message(label.scopedLabel()));
                        vertex.outs().put(CONDITION_POSITIVE, relation);
                        vertex.outs().put(CONDITION_POSITIVE, role);
                    } else {
                        TypeVertex type = graphMgr.schema().getType(label.label());
                        if (type == null) throw GraknException.of(TYPE_NOT_FOUND.message(label.label()));
                        vertex.outs().put(CONDITION_POSITIVE, type);
                    }
                });
    }

    private void putNegativeConditions() {
        vertex.outs().delete(CONDITION_NEGATIVE);
        when().negations().stream()
                .flatMap(negation -> negation.disjunction().conjunctions().stream())
                .flatMap(negatedConjunction -> negatedConjunction.constraints().stream())
                .filter(constraint -> constraint.isType() && constraint.asType().isLabel())
                .forEach(constraint -> {
                    LabelConstraint label = constraint.asType().asLabel();
                    if (label.scope().isPresent()) {
                        TypeVertex relation = graphMgr.schema().getType(label.scope().get());
                        TypeVertex role = graphMgr.schema().getType(label.label(), label.scope().get());
                        if (role == null) throw GraknException.of(TYPE_NOT_FOUND.message(label.scopedLabel()));
                        vertex.outs().put(CONDITION_NEGATIVE, relation);
                        vertex.outs().put(CONDITION_NEGATIVE, role);
                    } else {
                        TypeVertex type = graphMgr.schema().getType(label.label());
                        if (type == null) throw GraknException.of(TYPE_NOT_FOUND.message(label.label()));
                        vertex.outs().put(CONDITION_NEGATIVE, type);
                    }
                });
    }

    private void putConclusions() {
        vertex.outs().delete(CONCLUSION);
        then().stream()
                .flatMap(var -> var.constraints().stream())
                .filter(Constraint::isThing)
                .map(Constraint::asThing)
                .forEach(constraint -> {
                    if (constraint.isHas()) constraint.asHas().type().label().ifPresent(l -> putConclusion(l.label()));
                    else if (constraint.isIsa()) constraint.asIsa().type().label().ifPresent(l -> putConclusion(l.label()));
                    else if (constraint.isRelation()) putRelationConclusion(constraint.asRelation());
                });
    }

    private void putConclusion(String typeLabel) {
        TypeVertex type = graphMgr.schema().getType(typeLabel);
        if (type == null) throw GraknException.of(TYPE_NOT_FOUND.message(type));
        vertex.outs().put(CONCLUSION, type);
    }

    private void putRelationConclusion(RelationConstraint relation) {
        String relationLabel = relation.owner().isa().iterator().next().type().label().get().label();
        TypeVertex relationVertex = graphMgr.schema().getType(relationLabel);
        if (relationVertex == null) throw GraknException.of(TYPE_NOT_FOUND.message(relationLabel));
        RelationTypeImpl relationConcept = RelationTypeImpl.of(graphMgr, relationVertex);
        relation.asRelation().players().forEach(player -> {
            if (player.roleType().isPresent() && player.roleType().get().label().isPresent()) {
                final String roleLabel = player.roleType().get().label().get().label();
                RoleTypeImpl role = relationConcept.getRelates(roleLabel);
                if (role == null) throw GraknException.of(TYPE_NOT_FOUND.message(player.roleType().get().label().get().scopedLabel()));
                vertex.outs().put(CONCLUSION, role.vertex);
            }
        });
    }

    @Override
    public String getLabel() {
        return vertex.label();
    }

    @Override
    public void setLabel(final String label) {
        vertex.label(label);
    }

    @Override
    public graql.lang.pattern.Conjunction<? extends graql.lang.pattern.Pattern> getWhenPreNormalised() {
        return vertex.when();
    }

    @Override
    public graql.lang.pattern.variable.ThingVariable<?> getThenPreNormalised() {
        return vertex.then();
    }

    @Override
    public Conjunction when() {
        if (when == null) {
            when = Conjunction.create(getWhenPreNormalised().normalise().patterns().get(0));
        }
        return when;
    }

    @Override
    public Set<Variable> then() {
        if (then == null) {
            then = ThingVariable.createFromThings(Collections.singletonList(getThenPreNormalised()));
        }
        return then;
    }

    /**
     * TODO: Check logical validity of this rule
     * 1. that there are no nested negations
     * 2. that there are no disjunctions
     * 3. (in general, that the rules follow the allowed form)
     * 4. that all types referenced in the rule exist
     * 5. that the rule is satisfiable (eg. use type inference, check that there is one answer ie. a type for each var)
     * NOTE: this would imply that `//concept` depends on `//query` or `//traversal` to perform a query
     * which would induce a cyclical dependency. To resolve this, one idea is to remove rule definition
     * from concept API, and only allow its definition through `define`
     * Then perform type inference in the `Definer.java`, which has access to higher level operations like type inf
     * 6. check that the rule does not cause cycles in the type graph with a negation in it
     * NOTE: this would imply that `//concept` depends on the future `RuleGraph` object. This means it should like
     * in `//concept` as well. Alternatively, we can leave it higher level, and again have this validation be
     * performed at the `Definer.java` level
     *
     * Overall, if we can centralise all rule validation here, it would be easier to read and understand
     * However, it may introduce some architectural issues
     */
    private void validate() {
    }

    @Override
    public Stream<TypeImpl> positiveConditionTypes() {
        return vertex.outs().edge(CONDITION_POSITIVE).to().map(v -> TypeImpl.of(graphMgr, v)).stream();
    }

    @Override
    public Stream<TypeImpl> negativeConditionTypes() {
        return vertex.outs().edge(CONDITION_NEGATIVE).to().map(v -> TypeImpl.of(graphMgr, v)).stream();
    }

    @Override
    public Stream<TypeImpl> conclusionTypes() {
        return vertex.outs().edge(CONCLUSION).to().map(v -> TypeImpl.of(graphMgr, v)).stream();
    }

    @Override
    public boolean isDeleted() {
        return vertex.isDeleted();
    }

    @Override
    public void delete() {
        vertex.delete();
    }

    @Override
    public boolean equals(final Object object) {
        if (this == object) return true;
        if (object == null || getClass() != object.getClass()) return false;

        final RuleImpl that = (RuleImpl) object;
        return this.vertex.equals(that.vertex);
    }

    @Override
    public final int hashCode() {
        return vertex.hashCode(); // does not need caching
    }
}
