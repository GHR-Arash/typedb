/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use std::{collections::HashMap, error::Error, fmt, sync::Arc};

use answer::variable::Variable;
use bytes::byte_array::ByteArray;
use encoding::{graph::thing::THING_VERTEX_MAX_LENGTH, value::value::Value};
use error::typedb_error;
use itertools::Itertools;
use storage::snapshot::{iterator::SnapshotIteratorError, SnapshotGetError};
use typeql::schema::definable::function::{
    Function, FunctionBlock, ReturnReduction, ReturnSingle, ReturnStatement, ReturnStream, Signature,
};

use crate::{
    pattern::{
        constraint::Constraint,
        variable_category::{VariableCategory, VariableOptionality},
        ParameterID,
    },
    pipeline::{function_signature::FunctionID, reduce::Reducer},
    RepresentationError,
};

pub mod block;
pub mod fetch;
pub mod function;
pub mod function_signature;
pub mod modifier;
pub mod reduce;

#[derive(Debug, Clone)]
pub enum FunctionReadError {
    FunctionNotFound { function_id: FunctionID },
    FunctionRetrieval { source: SnapshotGetError },
    FunctionsScan { source: Arc<SnapshotIteratorError> },
}

impl fmt::Display for FunctionReadError {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

impl Error for FunctionReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FunctionRetrieval { source } => Some(source),
            Self::FunctionsScan { source } => Some(source),
            Self::FunctionNotFound { .. } => None,
        }
    }
}

typedb_error! {
    pub FunctionRepresentationError(component = "Function representation", prefix = "FNR") {
        FunctionArgumentUnused(
            1,
            "Function argument variable '{argument_variable}' is unused.\nSource:\n{declaration}",
            argument_variable: String,
            declaration: Function
        ),
        StreamReturnVariableUnavailable(
            2,
            "Function return variable '{return_variable}' is not available or defined.\nSource:\n{declaration}", // TODO: formatted
            return_variable: String,
            declaration: ReturnStream
        ),
        SingleReturnVariableUnavailable(
            3,
            "Function return variable '{return_variable}' is not available or defined.\nSource:\n{declaration}", // TODO: formatted
            return_variable: String,
            declaration: ReturnSingle
        ),
        BlockDefinition(
            4,
            "Function pattern contains an error.\nSource:\n{declaration}",
            declaration: FunctionBlock,
            ( typedb_source : Box<RepresentationError>)
        ),
        ReturnReduction(
            5,
            "Error building representation of the return reduction.\nSource:\n{declaration}",
            declaration: ReturnReduction,
            ( typedb_source : Box<RepresentationError>)
        ),
        IllegalFetch(
            6,
            "Fetch clauses cannot be used inside of functions or function blocks that terminate in a 'return' statement.\nSource:\n{declaration}",
            declaration: FunctionBlock
        ),
        IllegalStages(
            7,
            "Functions may not contain write stages.\nSource:\n{declaration}",
            declaration: FunctionBlock
        ),
        InconsistentReturn(
            8,
            "The return statement in the body of the function did not match that in the signature. \nSignature: {signature}\nDefinition: {return_}",
            signature: Signature,
            return_: ReturnStatement
        ),
        IllegalKeywordAsIdentifier(
            9,
            "The reserved keyword \"{identifier}\" cannot be used as function name",
            identifier: String
        ),
    }
}

#[derive(Debug, Clone)]
pub struct VariableRegistry {
    variable_names: HashMap<Variable, String>,
    variable_id_allocator: u16,
    variable_categories: HashMap<Variable, (VariableCategory, VariableCategorySource)>,
    variable_optionality: HashMap<Variable, VariableOptionality>,
}

impl VariableRegistry {
    pub(crate) fn new() -> VariableRegistry {
        Self {
            variable_names: HashMap::new(),
            variable_id_allocator: 0,
            variable_categories: HashMap::new(),
            variable_optionality: HashMap::new(),
        }
    }

    fn register_variable_named(&mut self, name: String) -> Variable {
        let variable = self.allocate_variable(false);
        self.variable_names.insert(variable, name);
        variable
    }

    fn register_anonymous_variable(&mut self) -> Variable {
        self.allocate_variable(true)
    }

    fn allocate_variable(&mut self, anonymous: bool) -> Variable {
        let variable = if anonymous {
            Variable::new_anonymous(self.variable_id_allocator)
        } else {
            Variable::new(self.variable_id_allocator)
        };
        self.variable_id_allocator += 1;
        variable
    }

    pub fn set_assigned_value_variable_category(
        &mut self,
        variable: Variable,
        category: VariableCategory,
        source: Constraint<Variable>,
    ) -> Result<(), Box<RepresentationError>> {
        self.set_variable_category(variable, category, VariableCategorySource::Constraint(source))
    }

    fn set_variable_category(
        &mut self,
        variable: Variable,
        category: VariableCategory,
        source: VariableCategorySource,
    ) -> Result<(), Box<RepresentationError>> {
        let existing_category = self.variable_categories.get_mut(&variable);
        match existing_category {
            None => {
                self.variable_categories.insert(variable, (category, source));
                Ok(())
            }
            Some((existing_category, existing_source)) => {
                let narrowest = existing_category.narrowest(category);
                match narrowest {
                    None => Err(Box::new(RepresentationError::VariableCategoryMismatch {
                        variable_name: self
                            .variable_names
                            .get(&variable)
                            .cloned()
                            .unwrap_or_else(|| "$<INTERNAL>".to_owned()),
                        category_1: category,
                        // category_1_source: source,
                        category_2: *existing_category,
                        // category_2_source: existing_source.clone(),
                    })),
                    Some(narrowed) => {
                        if narrowed == *existing_category {
                            Ok(())
                        } else {
                            *existing_category = narrowed;
                            *existing_source = source;
                            Ok(())
                        }
                    }
                }
            }
        }
    }

    fn set_variable_is_optional(&mut self, variable: Variable, optional: bool) {
        match optional {
            true => self.variable_optionality.insert(variable, VariableOptionality::Optional),
            false => self.variable_optionality.remove(&variable),
        };
    }

    pub fn variable_categories(&self) -> impl Iterator<Item = (Variable, VariableCategory)> + '_ {
        self.variable_categories.iter().map(|(&variable, &(category, _))| (variable, category))
    }

    pub fn variable_names(&self) -> &HashMap<Variable, String> {
        &self.variable_names
    }

    pub fn get_variable_name(&self, variable: Variable) -> Option<&String> {
        self.variable_names.get(&variable)
    }

    pub fn get_variable_category(&self, variable: Variable) -> Option<VariableCategory> {
        self.variable_categories.get(&variable).map(|(category, _constraint)| *category)
    }

    pub fn get_variable_optionality(&self, variable: Variable) -> Option<VariableOptionality> {
        self.variable_optionality.get(&variable).cloned()
    }

    pub(crate) fn is_variable_optional(&self, variable: Variable) -> bool {
        match self.variable_optionality.get(&variable).unwrap_or(&VariableOptionality::Required) {
            VariableOptionality::Required => false,
            VariableOptionality::Optional => true,
        }
    }

    pub fn has_variable_as_named(&self, variable: &Variable) -> bool {
        self.variable_names.contains_key(variable)
    }

    pub(crate) fn register_function_argument(&mut self, name: &str, category: VariableCategory) -> Variable {
        let variable = self.register_variable_named(name.to_owned());
        self.set_variable_category(variable, category, VariableCategorySource::Argument).unwrap(); // We just created the variable. It cannot error
        self.set_variable_is_optional(variable, false);
        variable
    }

    pub(crate) fn register_reduce_output_variable(
        &mut self,
        name: String,
        category: VariableCategory,
        is_optional: bool,
        reducer: Reducer,
    ) -> Variable {
        let variable = self.register_variable_named(name);
        self.set_variable_category(variable, category, VariableCategorySource::Reduce(reducer)).unwrap(); // We just created the variable. It cannot error
        self.set_variable_is_optional(variable, is_optional);
        variable
    }
}

impl fmt::Display for VariableRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Named variables:")?;
        for var in self.variable_names.keys().sorted_unstable() {
            writeln!(f, "  {}: ${}", var, self.variable_names[var])?;
        }
        writeln!(f, "Variable categories:")?;
        for var in self.variable_categories.keys().sorted_unstable() {
            writeln!(f, "  {}: {}", var, self.variable_categories[var].0)?;
        }
        writeln!(f, "Optional variables:")?;
        for var in self.variable_optionality.keys().sorted_unstable() {
            writeln!(f, "  {}", var)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum VariableCategorySource {
    Constraint(Constraint<Variable>),
    Reduce(Reducer),
    Argument,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParameterRegistry {
    value_registry: HashMap<ParameterID, Value<'static>>,
    iid_registry: HashMap<ParameterID, ByteArray<THING_VERTEX_MAX_LENGTH>>,
    fetch_key_registry: HashMap<ParameterID, String>,
}

impl ParameterRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn register_value(&mut self, value: Value<'static>) -> ParameterID {
        let id = ParameterID::Value(self.value_registry.len());
        let _prev = self.value_registry.insert(id, value);
        debug_assert_eq!(_prev, None);
        id
    }

    pub(crate) fn register_iid(&mut self, iid: ByteArray<THING_VERTEX_MAX_LENGTH>) -> ParameterID {
        let id = ParameterID::Iid(self.iid_registry.len());
        let _prev = self.iid_registry.insert(id, iid);
        debug_assert_eq!(_prev, None);
        id
    }

    pub(crate) fn register_fetch_key(&mut self, key: String) -> ParameterID {
        let id = ParameterID::FetchKey(self.fetch_key_registry.len());
        let _prev = self.fetch_key_registry.insert(id, key);
        debug_assert_eq!(_prev, None);
        id
    }

    pub fn value(&self, id: ParameterID) -> Option<&Value<'static>> {
        self.value_registry.get(&id)
    }

    pub fn value_unchecked(&self, id: ParameterID) -> &Value<'static> {
        self.value_registry.get(&id).unwrap()
    }

    pub fn iid(&self, id: ParameterID) -> Option<&ByteArray<THING_VERTEX_MAX_LENGTH>> {
        self.iid_registry.get(&id)
    }

    pub fn fetch_key(&self, id: ParameterID) -> Option<&String> {
        self.fetch_key_registry.get(&id)
    }
}
