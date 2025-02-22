/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use std::{
    collections::{HashMap, HashSet},
    iter::zip,
    sync::Arc,
};

use answer::variable::Variable;
use concept::thing::statistics::Statistics;
use ir::pipeline::{function_signature::FunctionID, reduce::AssignedReduction, VariableRegistry};
use itertools::Itertools;

use crate::{
    annotation::{
        fetch::AnnotatedFetch,
        function::{AnnotatedPreambleFunctions, AnnotatedSchemaFunctions},
        pipeline::AnnotatedStage,
    },
    executable::{
        delete::executable::DeleteExecutable,
        fetch::executable::{compile_fetch, ExecutableFetch},
        function::{compile_function, determine_tabling_requirements},
        insert::executable::InsertExecutable,
        match_::planner::{function_plan::ExecutableFunctionRegistry, match_executable::MatchExecutable},
        modifiers::{LimitExecutable, OffsetExecutable, RequireExecutable, SelectExecutable, SortExecutable},
        reduce::{ReduceExecutable, ReduceRowsExecutable},
        ExecutableCompilationError,
    },
    VariablePosition,
};

#[derive(Debug, Clone)]
pub struct ExecutablePipeline {
    pub executable_functions: ExecutableFunctionRegistry,
    pub executable_stages: Vec<ExecutableStage>,
    pub executable_fetch: Option<Arc<ExecutableFetch>>,
}

#[derive(Debug, Clone)]
pub enum ExecutableStage {
    Match(Arc<MatchExecutable>),
    Insert(Arc<InsertExecutable>),
    Delete(Arc<DeleteExecutable>),

    Select(Arc<SelectExecutable>),
    Sort(Arc<SortExecutable>),
    Offset(Arc<OffsetExecutable>),
    Limit(Arc<LimitExecutable>),
    Require(Arc<RequireExecutable>),
    Reduce(Arc<ReduceExecutable>),
}

impl ExecutableStage {
    pub fn output_row_mapping(&self) -> HashMap<Variable, VariablePosition> {
        match self {
            ExecutableStage::Match(executable) => executable.variable_positions().to_owned(),
            ExecutableStage::Insert(executable) => executable
                .output_row_schema
                .iter()
                .enumerate()
                .filter_map(|(i, opt)| opt.map(|(v, _)| (i, v)))
                .map(|(i, v)| (v, VariablePosition::new(i as u32)))
                .collect(),
            ExecutableStage::Delete(executable) => executable
                .output_row_schema
                .iter()
                .enumerate()
                .filter_map(|(i, v)| v.map(|v| (v, VariablePosition::new(i as u32))))
                .collect(),
            ExecutableStage::Select(executable) => executable.output_row_mapping.clone(),
            ExecutableStage::Sort(executable) => executable.output_row_mapping.clone(),
            ExecutableStage::Offset(executable) => executable.output_row_mapping.clone(),
            ExecutableStage::Limit(executable) => executable.output_row_mapping.clone(),
            ExecutableStage::Require(executable) => executable.output_row_mapping.clone(),
            ExecutableStage::Reduce(executable) => executable.output_row_mapping.clone(),
        }
    }
}

pub fn compile_pipeline(
    statistics: &Statistics,
    variable_registry: &VariableRegistry,
    annotated_schema_functions: &AnnotatedSchemaFunctions,
    annotated_preamble: AnnotatedPreambleFunctions,
    annotated_stages: Vec<AnnotatedStage>,
    annotated_fetch: Option<AnnotatedFetch>,
    input_variables: &HashSet<Variable>,
) -> Result<ExecutablePipeline, ExecutableCompilationError> {
    // TODO: Cache compiled schema functions?
    let mut executable_schema_functions = HashMap::new();
    let schema_tabling_requirements = determine_tabling_requirements(
        &annotated_schema_functions.iter().map(|(id, function)| (FunctionID::Schema(id.clone()), function)).collect(),
    );
    for (id, function) in annotated_schema_functions.iter() {
        // TODO: We could save cloning the whole function and only clone the stages.
        let compiled = compile_function(
            statistics,
            &ExecutableFunctionRegistry::empty(),
            function.clone(),
            schema_tabling_requirements[&FunctionID::Schema(id.clone())],
        )?;
        executable_schema_functions.insert(id.clone(), compiled);
    }
    let arced_executable_schema_functions = Arc::new(executable_schema_functions);
    let schema_function_registry =
        ExecutableFunctionRegistry::new(arced_executable_schema_functions.clone(), HashMap::new());

    let preamble_tabling_requirements = determine_tabling_requirements(
        &annotated_preamble.iter().enumerate().map(|(id, function)| (FunctionID::Preamble(id), function)).collect(),
    );
    let mut executable_preamble_functions = HashMap::new();
    for (id, function) in annotated_preamble.into_iter().enumerate() {
        let compiled = compile_function(
            statistics,
            &schema_function_registry,
            function,
            preamble_tabling_requirements[&FunctionID::Preamble(id)],
        )?;
        executable_preamble_functions.insert(id, compiled);
    }

    let schema_and_preamble_functions: ExecutableFunctionRegistry =
        ExecutableFunctionRegistry::new(arced_executable_schema_functions, executable_preamble_functions);
    let (_input_positions, executable_stages, executable_fetch) = compile_stages_and_fetch(
        statistics,
        variable_registry,
        &schema_and_preamble_functions,
        annotated_stages,
        annotated_fetch,
        input_variables,
    )?;
    Ok(ExecutablePipeline { executable_functions: schema_and_preamble_functions, executable_stages, executable_fetch })
}

pub fn compile_stages_and_fetch(
    statistics: &Statistics,
    variable_registry: &VariableRegistry,
    available_functions: &ExecutableFunctionRegistry,
    annotated_stages: Vec<AnnotatedStage>,
    annotated_fetch: Option<AnnotatedFetch>,
    input_variables: &HashSet<Variable>,
) -> Result<
    (HashMap<Variable, VariablePosition>, Vec<ExecutableStage>, Option<Arc<ExecutableFetch>>),
    ExecutableCompilationError,
> {
    let selected_variables = variable_registry.variable_names().keys().copied().collect_vec();
    let (input_positions, executable_stages) = compile_pipeline_stages(
        statistics,
        variable_registry,
        available_functions,
        annotated_stages,
        input_variables.iter().cloned(),
        &selected_variables,
    )?;
    let stages_variable_positions =
        executable_stages.last().map(|stage: &ExecutableStage| stage.output_row_mapping()).unwrap_or(HashMap::new());

    let executable_fetch = annotated_fetch
        .map(|fetch| {
            compile_fetch(statistics, available_functions, fetch, &stages_variable_positions)
                .map_err(|err| ExecutableCompilationError::FetchCompliation { typedb_source: err })
        })
        .transpose()?
        .map(Arc::new);
    Ok((input_positions, executable_stages, executable_fetch))
}

pub(crate) fn compile_pipeline_stages(
    statistics: &Statistics,
    variable_registry: &VariableRegistry,
    functions: &ExecutableFunctionRegistry,
    annotated_stages: Vec<AnnotatedStage>,
    input_variables: impl Iterator<Item = Variable>,
    selected_variables: &[Variable],
) -> Result<(HashMap<Variable, VariablePosition>, Vec<ExecutableStage>), ExecutableCompilationError> {
    let mut executable_stages: Vec<ExecutableStage> = Vec::with_capacity(annotated_stages.len());
    let input_variable_positions =
        input_variables.enumerate().map(|(i, var)| (var, VariablePosition::new(i as u32))).collect();

    for stage in annotated_stages {
        // TODO: We can filter out the variables that are no longer needed in the future stages, but are carried as selected variables from the previous one
        let selected_variables = selected_variables
            .iter()
            .cloned()
            .chain(stage.named_referenced_variables(variable_registry))
            .unique()
            .collect_vec();
        let executable_stage = match executable_stages.last().map(|stage| stage.output_row_mapping()) {
            Some(row_mapping) => {
                compile_stage(statistics, variable_registry, functions, &row_mapping, &selected_variables, stage)?
            }
            None => compile_stage(
                statistics,
                variable_registry,
                functions,
                &input_variable_positions,
                &selected_variables,
                stage,
            )?,
        };
        executable_stages.push(executable_stage);
    }
    Ok((input_variable_positions, executable_stages))
}

fn compile_stage(
    statistics: &Statistics,
    variable_registry: &VariableRegistry,
    _functions: &ExecutableFunctionRegistry,
    input_variables: &HashMap<Variable, VariablePosition>,
    selected_variables: &[Variable],
    annotated_stage: AnnotatedStage,
) -> Result<ExecutableStage, ExecutableCompilationError> {
    match &annotated_stage {
        AnnotatedStage::Match { block, block_annotations, executable_expressions } => {
            let plan = crate::executable::match_::planner::compile(
                block,
                input_variables,
                selected_variables,
                block_annotations,
                variable_registry,
                executable_expressions,
                statistics,
            );
            Ok(ExecutableStage::Match(Arc::new(plan)))
        }
        AnnotatedStage::Insert { block, annotations } => {
            let plan = crate::executable::insert::executable::compile(
                block.conjunction().constraints(),
                input_variables,
                annotations,
            )
            .map_err(|source| ExecutableCompilationError::InsertExecutableCompilation { source })?;
            Ok(ExecutableStage::Insert(Arc::new(plan)))
        }
        AnnotatedStage::Delete { block, deleted_variables, annotations } => {
            let plan = crate::executable::delete::executable::compile(
                input_variables,
                annotations,
                block.conjunction().constraints(),
                deleted_variables,
            )
            .map_err(|source| ExecutableCompilationError::DeleteExecutableCompilation { source })?;
            Ok(ExecutableStage::Delete(Arc::new(plan)))
        }
        AnnotatedStage::Select(select) => {
            let mut retained_positions = HashSet::with_capacity(select.variables.len());
            let mut output_row_mapping = HashMap::with_capacity(select.variables.len());
            for &variable in &select.variables {
                let pos = input_variables[&variable];
                retained_positions.insert(pos);
                output_row_mapping.insert(variable, pos);
            }
            Ok(ExecutableStage::Select(Arc::new(SelectExecutable::new(retained_positions, output_row_mapping))))
        }
        AnnotatedStage::Sort(sort) => {
            Ok(ExecutableStage::Sort(Arc::new(SortExecutable::new(sort.variables.clone(), input_variables.clone()))))
        }
        AnnotatedStage::Offset(offset) => {
            Ok(ExecutableStage::Offset(Arc::new(OffsetExecutable::new(offset.offset(), input_variables.clone()))))
        }
        AnnotatedStage::Limit(limit) => {
            Ok(ExecutableStage::Limit(Arc::new(LimitExecutable::new(limit.limit(), input_variables.clone()))))
        }
        AnnotatedStage::Require(require) => {
            let mut required_positions = HashSet::with_capacity(require.variables.len());
            for &variable in &require.variables {
                let pos = input_variables[&variable];
                required_positions.insert(pos);
            }
            Ok(ExecutableStage::Require(Arc::new(RequireExecutable::new(required_positions, input_variables.clone()))))
        }
        AnnotatedStage::Reduce(reduce, typed_reducers) => {
            debug_assert_eq!(reduce.assigned_reductions.len(), typed_reducers.len());
            let mut output_row_mapping = HashMap::new();
            let mut input_group_positions = Vec::with_capacity(reduce.within_group.len());
            for variable in reduce.within_group.iter() {
                output_row_mapping.insert(*variable, VariablePosition::new(input_group_positions.len() as u32));
                input_group_positions.push(input_variables[variable]);
            }
            let mut reductions = Vec::with_capacity(reduce.assigned_reductions.len());
            for (&AssignedReduction { assigned, .. }, reducer_on_variable) in
                zip(reduce.assigned_reductions.iter(), typed_reducers.iter())
            {
                output_row_mapping
                    .insert(assigned, VariablePosition::new((input_group_positions.len() + reductions.len()) as u32));
                let reducer_on_position = reducer_on_variable.clone().map(input_variables);
                reductions.push(reducer_on_position);
            }
            Ok(ExecutableStage::Reduce(Arc::new(ReduceExecutable::new(
                ReduceRowsExecutable { reductions, input_group_positions },
                output_row_mapping,
            ))))
        }
    }
}
