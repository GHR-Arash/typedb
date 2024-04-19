/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use std::cmp::Ordering;
use std::sync::Arc;
use iterator::State;

use bytes::Bytes;
use encoding::{
    AsBytes,
    graph::{thing::vertex_attribute::AttributeVertex, type_::vertex::build_vertex_attribute_type, Typed},
    Keyable, value::value_type::ValueType,
};
use encoding::graph::thing::edge::ThingEdgeHasReverse;
use encoding::value::decode_value_u64;
use storage::{
    key_value::StorageKeyReference,
    snapshot::{ReadableSnapshot, WritableSnapshot},
};
use storage::snapshot::iterator::{SnapshotIteratorError, SnapshotRangeIterator};

use crate::{ByteReference, ConceptAPI, ConceptStatus, edge_iterator, error::{ConceptReadError, ConceptWriteError}, GetStatus, thing::{thing_manager::ThingManager, ThingAPI, value::Value}, type_::attribute_type::AttributeType};
use crate::thing::object::Object;
use crate::type_::type_manager::TypeManager;

#[derive(Debug)]
pub struct Attribute<'a> {
    vertex: AttributeVertex<'a>,
    value: Option<Value<'a>>, // TODO: if we end up doing traversals over Vertex instead of Concept, we could embed the Value cache into the AttributeVertex
}

impl<'a> Attribute<'a> {
    pub(crate) fn new(vertex: AttributeVertex<'a>) -> Self {
        Attribute { vertex, value: None }
    }

    pub(crate) fn value_type(&self) -> ValueType {
        self.vertex.value_type()
    }

    pub fn type_(&self) -> AttributeType<'static> {
        AttributeType::new(build_vertex_attribute_type(self.vertex.type_id_()))
    }

    pub fn iid(&self) -> ByteReference<'_> {
        self.vertex.bytes()
    }

    pub fn value(
        &mut self,
        thing_manager: &ThingManager<impl ReadableSnapshot>,
    ) -> Result<Value<'_>, ConceptReadError> {
        if self.value.is_none() {
            let value = thing_manager.get_attribute_value(self)?;
            self.value = Some(value);
        }
        Ok(self.value.as_ref().unwrap().as_reference())
    }

    pub fn has_owners<'m>(&self, thing_manager: &'m ThingManager<impl ReadableSnapshot>) -> bool {
        match self.get_status(thing_manager) {
            ConceptStatus::Put => thing_manager.has_owners(self.as_reference(), false),
            ConceptStatus::Inserted | ConceptStatus::Persisted | ConceptStatus::Deleted => {
                unreachable!("Attributes are expected to always have a PUT status.")
            }
        }
    }

    pub fn get_owners<'m>(
        &self, thing_manager: &'m ThingManager<impl ReadableSnapshot>,
    ) -> AttributeOwnerIterator<'m, { ThingEdgeHasReverse::LENGTH_BOUND_PREFIX_FROM }> {
        thing_manager.get_owners_of(self.as_reference())
    }

    pub fn as_reference(&self) -> Attribute<'_> {
        Attribute { vertex: self.vertex.as_reference(), value: self.value.as_ref().map(|value| value.as_reference()) }
    }

    pub(crate) fn vertex<'this: 'a>(&'this self) -> AttributeVertex<'this> {
        self.vertex.as_reference()
    }

    pub(crate) fn into_vertex(self) -> AttributeVertex<'a> {
        self.vertex
    }

    pub(crate) fn into_owned(self) -> Attribute<'static> {
        Attribute::new(self.vertex.into_owned())
    }
}

impl<'a> ConceptAPI<'a> for Attribute<'a> {}

impl<'a> ThingAPI<'a> for Attribute<'a> {
    fn set_modified(&self, thing_manager: &ThingManager<impl WritableSnapshot>) {
        // Attributes are always PUT, so we don't have to record a lock on modification
    }

    fn get_status<'m>(&self, thing_manager: &'m ThingManager<impl ReadableSnapshot>) -> ConceptStatus {
        debug_assert_eq!(thing_manager.get_status(self.vertex().as_storage_key()), ConceptStatus::Put);
        ConceptStatus::Put
    }

    fn errors(&self, thing_manager: &ThingManager<impl WritableSnapshot>) -> Result<Vec<ConceptWriteError>, ConceptReadError> {
        Ok(Vec::new())
    }

    fn delete<'m>(self, thing_manager: &'m ThingManager<impl WritableSnapshot>) -> Result<(), ConceptWriteError> {
        let mut owner_iter = self.get_owners(thing_manager);
        let mut owner = owner_iter.next().transpose()
            .map_err(|err| ConceptWriteError::ConceptRead { source: err })?;
        while let Some((object, count)) = owner {
            object.delete_has_many(thing_manager, self.as_reference(), count)?;
            owner = owner_iter.next().transpose()
                .map_err(|err| ConceptWriteError::ConceptRead { source: err })?;
        }

        thing_manager.delete_attribute(self);
        Ok(())
    }
}

impl<'a> PartialEq<Self> for Attribute<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.vertex().eq(&other.vertex())
    }
}

impl<'a> Eq for Attribute<'a> {}

impl<'a> PartialOrd<Self> for Attribute<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a> Ord for Attribute<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.vertex.cmp(&other.vertex())
    }
}

///
/// Attribute iterators handle hiding dependent attributes that were not deleted yet
///
pub struct AttributeIterator<'a, Snapshot: ReadableSnapshot, const A_PS: usize, const H_PS: usize> {
    type_manager: Option<&'a TypeManager<Snapshot>>,
    attributes_iterator: Option<SnapshotRangeIterator<'a, A_PS>>,
    has_reverse_iterator: Option<SnapshotRangeIterator<'a, H_PS>>,
    state: State<ConceptReadError>,
}

impl<'a, Snapshot: ReadableSnapshot, const A_PS: usize, const H_PS: usize> AttributeIterator<'a, Snapshot, A_PS, H_PS> {
    pub(crate) fn new(
        attributes_iterator: SnapshotRangeIterator<'a, A_PS>, has_reverse_iterator: SnapshotRangeIterator<'a, H_PS>,
        type_manager: &'a TypeManager<Snapshot>,
    ) -> Self {
        Self {
            type_manager: Some(type_manager),
            attributes_iterator: Some(attributes_iterator),
            has_reverse_iterator: Some(has_reverse_iterator),
            state: State::Init
        }
    }

    pub(crate) fn new_empty() -> Self {
        Self { type_manager: None, attributes_iterator: None, has_reverse_iterator: None, state: State::Done }
    }

    fn storage_key_to_attribute<'b>(storage_key_ref: StorageKeyReference<'b>) -> Attribute<'b> {
        Attribute::new(AttributeVertex::new(Bytes::Reference(storage_key_ref.byte_ref())))
    }

    pub fn peek(&mut self) -> Option<Result<Attribute<'_>, ConceptReadError>> {
        self.iter_peek().map(|result| {
            result
                .map(|(storage_key, _value_bytes)| Self::storage_key_to_attribute(storage_key))
                .map_err(|error| ConceptReadError::SnapshotIterate { source: error })
        })
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Result<Attribute<'_>, ConceptReadError>> {
        self.iter_next().map(|result| {
            result
                .map(|(storage_key, _value_bytes)| Self::storage_key_to_attribute(storage_key))
                .map_err(|error| ConceptReadError::SnapshotIterate { source: error })
        })
    }

    pub fn seek(&mut self) {
        todo!()
    }

    fn iter_peek(
        &mut self,
    ) -> Option<Result<(StorageKeyReference<'_>, ByteReference<'_>), Arc<SnapshotIteratorError>>>
    {
        if let Some(iter) = self.attributes_iterator.as_mut() {
            iter.peek()
        } else {
            None
        }
    }

    fn iter_next(
        &mut self,
    ) -> Option<Result<(StorageKeyReference<'_>, ByteReference<'_>), Arc<SnapshotIteratorError>>>
    {
        if let Some(iter) = self.attributes_iterator.as_mut() {
            iter.next()
        } else {
            None
        }
    }

    pub fn collect_cloned(mut self) -> Vec<Attribute<'static>> {
        let mut vec = Vec::new();
        loop {
            let item = self.next();
            if item.is_none() {
                break;
            }
            let key = item.unwrap().unwrap().into_owned();
            vec.push(key);
        }
        vec
    }
}


fn storage_key_to_owner<'a>(
    storage_key_reference: StorageKeyReference<'a>,
    value: ByteReference<'a>,
) -> (Object<'a>, u64) {
    let edge = ThingEdgeHasReverse::new(Bytes::Reference(storage_key_reference.byte_ref()));
    (Object::new(edge.into_to()), decode_value_u64(value))
}

edge_iterator!(
    AttributeOwnerIterator;
    (Object<'_>, u64);
    storage_key_to_owner
);