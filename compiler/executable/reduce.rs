/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use std::{collections::HashMap, sync::Arc};

use answer::variable::Variable;
use encoding::value::value_type::ValueType;
use ir::pattern::IrID;

use crate::{executable::next_executable_id, VariablePosition};

#[derive(Debug, Clone)]
pub struct ReduceExecutable {
    pub executable_id: u64,
    pub reduce_rows_executable: Arc<ReduceRowsExecutable>,
    pub output_row_mapping: HashMap<Variable, VariablePosition>, // output_row = (group_vars, reduce_outputs)
}

impl ReduceExecutable {
    pub(crate) fn new(
        rows_executable: ReduceRowsExecutable,
        output_row_mapping: HashMap<Variable, VariablePosition>,
    ) -> Self {
        Self {
            executable_id: next_executable_id(),
            reduce_rows_executable: Arc::new(rows_executable),
            output_row_mapping,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReduceRowsExecutable {
    pub reductions: Vec<ReduceInstruction<VariablePosition>>,
    pub input_group_positions: Vec<VariablePosition>,
}

#[derive(Debug, Clone)]
pub enum ReduceInstruction<ID: IrID> {
    Count,
    CountVar(ID),
    SumLong(ID),
    SumDouble(ID),
    MaxLong(ID),
    MaxDouble(ID),
    MinLong(ID),
    MinDouble(ID),
    MeanLong(ID),
    MeanDouble(ID),
    MedianLong(ID),
    MedianDouble(ID),
    StdLong(ID),
    StdDouble(ID),
}

impl<ID: IrID> ReduceInstruction<ID> {
    pub fn id(&self) -> Option<ID> {
        match *self {
            Self::Count => None,

            Self::CountVar(id)
            | Self::SumLong(id)
            | Self::SumDouble(id)
            | Self::MaxLong(id)
            | Self::MaxDouble(id)
            | Self::MinLong(id)
            | Self::MinDouble(id)
            | Self::MeanLong(id)
            | Self::MeanDouble(id)
            | Self::MedianLong(id)
            | Self::MedianDouble(id)
            | Self::StdLong(id)
            | Self::StdDouble(id) => Some(id),
        }
    }

    pub fn output_type(&self) -> ValueType {
        match self {
            Self::Count => ValueType::Long,
            Self::CountVar(_) => ValueType::Long,
            Self::SumLong(_) => ValueType::Long,
            Self::SumDouble(_) => ValueType::Double,
            Self::MaxLong(_) => ValueType::Long,
            Self::MaxDouble(_) => ValueType::Double,
            Self::MinLong(_) => ValueType::Long,
            Self::MinDouble(_) => ValueType::Double,
            Self::MeanLong(_) => ValueType::Double,
            Self::MeanDouble(_) => ValueType::Double,
            Self::MedianLong(_) => ValueType::Double,
            Self::MedianDouble(_) => ValueType::Double,
            Self::StdLong(_) => ValueType::Double,
            Self::StdDouble(_) => ValueType::Double,
        }
    }

    pub fn map<T: IrID>(self, mapping: &HashMap<ID, T>) -> ReduceInstruction<T> {
        match self {
            ReduceInstruction::Count => ReduceInstruction::Count,
            ReduceInstruction::CountVar(id) => ReduceInstruction::CountVar(mapping[&id]),
            ReduceInstruction::SumLong(id) => ReduceInstruction::SumLong(mapping[&id]),
            ReduceInstruction::SumDouble(id) => ReduceInstruction::SumDouble(mapping[&id]),
            ReduceInstruction::MaxLong(id) => ReduceInstruction::MaxLong(mapping[&id]),
            ReduceInstruction::MaxDouble(id) => ReduceInstruction::MaxDouble(mapping[&id]),
            ReduceInstruction::MinLong(id) => ReduceInstruction::MinLong(mapping[&id]),
            ReduceInstruction::MinDouble(id) => ReduceInstruction::MinDouble(mapping[&id]),
            ReduceInstruction::MeanLong(id) => ReduceInstruction::MeanLong(mapping[&id]),
            ReduceInstruction::MeanDouble(id) => ReduceInstruction::MeanDouble(mapping[&id]),
            ReduceInstruction::MedianLong(id) => ReduceInstruction::MedianLong(mapping[&id]),
            ReduceInstruction::MedianDouble(id) => ReduceInstruction::MedianDouble(mapping[&id]),
            ReduceInstruction::StdLong(id) => ReduceInstruction::StdLong(mapping[&id]),
            ReduceInstruction::StdDouble(id) => ReduceInstruction::StdDouble(mapping[&id]),
        }
    }
}
