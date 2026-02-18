use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;

use crate::expression;
use crate::model::{
    BoundaryEvent, EscalationBoundaryEvent, EventSubProcess, EventSubProcessSubscription,
    EventSubProcessTrigger, FlowElement, ProcessDefinition, TimerDefinition, VisitEvent,
    VisitedElement,
};
use crate::scripting::{RhaiEvaluator, ScriptEvaluator, ScriptInput};

#[derive(Debug, Clone, PartialEq)]
pub enum InstanceState {
    Running,
    WaitingForTask,
    WaitingForTimer {
        element_id: String,
        definition: TimerDefinition,
    },
    WaitingForMessage {
        element_id: String,
        message_name: String,
        correlation_key_expr: Option<String>,
    },
    WaitingForSignal {
        element_id: String,
        signal_ref: String,
    },
    Completed,
    Failed,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub element_id: String,
}

#[derive(Debug, Clone)]
pub enum WaitState {
    Task {
        topic: Option<String>,
    },
    Timer {
        definition: TimerDefinition,
    },
    Message {
        message_name: String,
        correlation_key_expr: Option<String>,
    },
    Signal {
        signal_ref: String,
    },
}

#[derive(Debug, Clone)]
pub struct ActiveElement {
    pub element_id: String,
    pub wait_state: WaitState,
}

#[derive(Debug)]
pub struct ExecutionResult {
    pub active_elements: Vec<ActiveElement>,
    pub variables: HashMap<String, Value>,
    /// (element_id, message_name) pairs thrown during this step by MessageEndEvent or MessageIntermediateThrowEvent
    pub thrown_messages: Vec<(String, String)>,
    /// Error code thrown by an ErrorEndEvent (for subprocess error boundary matching)
    pub thrown_error: Option<String>,
    /// Escalation code thrown by EscalationEndEvent/EscalationIntermediateThrowEvent
    pub thrown_escalation: Option<String>,
    /// Elements visited during this execution step, in traversal order
    pub visited: Vec<VisitedElement>,
    /// Event subprocess subscriptions to persist (Message, Signal, Timer triggers only)
    pub event_subprocess_subscriptions: Vec<EventSubProcessSubscription>,
    pub is_completed: bool,
    pub is_failed: bool,
}

impl ExecutionResult {
    /// Temporary: derive a single InstanceState for backwards compat during migration.
    /// Remove after Task 2 is complete.
    pub fn compat_state(&self) -> InstanceState {
        if self.is_completed {
            return InstanceState::Completed;
        }
        if self.is_failed {
            return InstanceState::Failed;
        }
        match self.active_elements.first() {
            None => InstanceState::Running,
            Some(e) => match &e.wait_state {
                WaitState::Task { .. } => InstanceState::WaitingForTask,
                WaitState::Timer { definition } => InstanceState::WaitingForTimer {
                    element_id: e.element_id.clone(),
                    definition: definition.clone(),
                },
                WaitState::Message {
                    message_name,
                    correlation_key_expr,
                } => InstanceState::WaitingForMessage {
                    element_id: e.element_id.clone(),
                    message_name: message_name.clone(),
                    correlation_key_expr: correlation_key_expr.clone(),
                },
                WaitState::Signal { signal_ref } => InstanceState::WaitingForSignal {
                    element_id: e.element_id.clone(),
                    signal_ref: signal_ref.clone(),
                },
            },
        }
    }

    /// Temporary: return element IDs only for backwards compat.
    pub fn active_element_ids(&self) -> Vec<String> {
        self.active_elements
            .iter()
            .map(|e| e.element_id.clone())
            .collect()
    }
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("No start event found in process definition")]
    NoStartEvent,
    #[error("No outgoing sequence flow from element '{0}'")]
    NoOutgoingFlow(String),
    #[error("Sequence flow '{0}' target not found")]
    TargetNotFound(String),
    #[error("No active task to complete")]
    NoActiveTask,
    #[error("Task '{0}' is not the current waiting task")]
    WrongTask(String),
    #[error("ExclusiveGateway '{0}': no outgoing flow condition matched and no default flow")]
    NoMatchingCondition(String),
    #[error("Unsupported element type: {0}")]
    UnsupportedElement(String),
    #[error("Script failed at element '{element_id}': {message}")]
    ScriptFailed {
        element_id: String,
        message: String,
        line: Option<usize>,
    },
    #[error("Unsupported script language '{language}' at element '{element_id}'")]
    UnsupportedScriptLanguage {
        element_id: String,
        language: String,
    },
}

/// A frame on the subprocess execution stack
struct SubprocessFrame {
    /// The subprocess element ID in the parent process
    subprocess_element_id: String,
    /// Child engine running the inner process
    child: Box<Engine>,
}

pub struct Engine {
    definition: ProcessDefinition,
    state: InstanceState,
    pub tokens: Vec<Token>,
    pub variables: HashMap<String, Value>,
    /// Counts how many tokens have arrived at each join gateway
    join_counts: HashMap<String, usize>,
    /// Active multi-instance loops: element_id → (current_index, total_count)
    loop_state: HashMap<String, (usize, usize)>,
    /// Inclusive gateway join: gateway_id → number of branches that were activated at fork time
    inclusive_join_counts: HashMap<String, usize>,
    /// Active subprocess frames (innermost is last)
    subprocess_stack: Vec<SubprocessFrame>,
    /// Messages thrown during the current execution step: (element_id, message_name)
    thrown_messages: Vec<(String, String)>,
    /// Error code thrown by an ErrorEndEvent (for subprocess error boundary matching)
    thrown_error: Option<String>,
    /// Escalation code thrown by an EscalationEndEvent/EscalationIntermediateThrowEvent (for subprocess escalation boundary matching)
    thrown_escalation: Option<String>,
    /// Elements visited during this execution step, in traversal order
    visited: Vec<VisitedElement>,
    script_evaluators: HashMap<String, Box<dyn ScriptEvaluator>>,
}

impl Engine {
    pub fn new(definition: ProcessDefinition) -> Self {
        let mut evaluators: HashMap<String, Box<dyn ScriptEvaluator>> = HashMap::new();
        let rhai = RhaiEvaluator::new();
        evaluators.insert(rhai.language().to_string(), Box::new(rhai));
        Self {
            definition,
            state: InstanceState::Running,
            tokens: Vec::new(),
            variables: HashMap::new(),
            join_counts: HashMap::new(),
            loop_state: HashMap::new(),
            inclusive_join_counts: HashMap::new(),
            subprocess_stack: Vec::new(),
            thrown_messages: Vec::new(),
            thrown_error: None,
            thrown_escalation: None,
            visited: Vec::new(),
            script_evaluators: evaluators,
        }
    }

    pub fn definition(&self) -> &ProcessDefinition {
        &self.definition
    }

    pub fn start(
        &mut self,
        variables: HashMap<String, Value>,
    ) -> Result<ExecutionResult, EngineError> {
        self.visited.clear();
        self.variables = variables;

        // Restore join_counts from variables if present (crash recovery)
        if let Some(jc) = self.variables.remove("__join_counts__") {
            if let Ok(counts) = serde_json::from_value::<HashMap<String, usize>>(jc) {
                self.join_counts = counts;
            }
        }
        // Restore loop_state from variables if present (crash recovery)
        if let Some(ls) = self.variables.remove("__loop_state__") {
            if let Ok(state) = serde_json::from_value::<HashMap<String, (usize, usize)>>(ls) {
                self.loop_state = state;
            }
        }
        // Restore inclusive_join_counts from variables if present (crash recovery)
        if let Some(ijc) = self.variables.remove("__inclusive_join_counts__") {
            if let Ok(counts) = serde_json::from_value::<HashMap<String, usize>>(ijc) {
                self.inclusive_join_counts = counts;
            }
        }

        let start_id = self
            .definition
            .elements
            .iter()
            .find(|e| {
                matches!(
                    e,
                    FlowElement::StartEvent(_)
                        | FlowElement::TimerStartEvent(_)
                        | FlowElement::MessageStartEvent(_)
                        | FlowElement::SignalStartEvent(_)
                )
            })
            .map(|e| e.id().to_string())
            .ok_or(EngineError::NoStartEvent)?;

        self.advance_from(&start_id)?;

        Ok(self.result())
    }

    /// Restore engine state from persisted `active_element_ids` instead of replaying from start.
    /// Use this when rebuilding the engine to process a subsequent task in a multi-step workflow.
    pub fn resume(
        &mut self,
        variables: HashMap<String, Value>,
        active_element_ids: Vec<String>,
    ) -> Result<ExecutionResult, EngineError> {
        self.variables = variables;
        if let Some(jc) = self.variables.remove("__join_counts__") {
            if let Ok(counts) = serde_json::from_value::<HashMap<String, usize>>(jc) {
                self.join_counts = counts;
            }
        }
        if let Some(ls) = self.variables.remove("__loop_state__") {
            if let Ok(state) = serde_json::from_value::<HashMap<String, (usize, usize)>>(ls) {
                self.loop_state = state;
            }
        }
        if let Some(ijc) = self.variables.remove("__inclusive_join_counts__") {
            if let Ok(counts) = serde_json::from_value::<HashMap<String, usize>>(ijc) {
                self.inclusive_join_counts = counts;
            }
        }
        for eid in active_element_ids {
            self.tokens.push(Token { element_id: eid });
        }

        // Rebuild subprocess frames for tokens that belong to ESP inner elements.
        // When the engine is reconstructed from DB state, tokens for ESP inner tasks
        // (e.g. "esp_task") exist in self.tokens but subprocess_stack is empty.
        // We rebuild the frames so complete_task / fire_timer / etc. can delegate correctly.
        let mut esp_ids_rebuilt: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let token_ids: Vec<String> = self.tokens.iter().map(|t| t.element_id.clone()).collect();
        for eid in &token_ids {
            if self.find_element(eid).is_some() {
                continue; // top-level element — no subprocess frame needed
            }
            // Find which root-level ESP contains this element
            let esp_match = self.definition.elements.iter().find_map(|e| match e {
                FlowElement::EventSubProcess(esp) => {
                    if esp.inner_elements.iter().any(|ie| ie.id() == eid.as_str()) {
                        Some(esp.clone())
                    } else {
                        None
                    }
                }
                _ => None,
            });
            if let Some(esp) = esp_match {
                if esp_ids_rebuilt.contains(&esp.id) {
                    continue; // frame already created for this ESP
                }
                // Collect all tokens that belong to this ESP's inner elements
                let inner_active: Vec<String> = token_ids
                    .iter()
                    .filter(|t| esp.inner_elements.iter().any(|ie| ie.id() == t.as_str()))
                    .cloned()
                    .collect();
                let inner_def = crate::model::ProcessDefinition {
                    id: format!("{}_esp", esp.id),
                    name: None,
                    elements: esp.inner_elements.clone(),
                    sequence_flows: esp.inner_flows.clone(),
                    annotations: vec![],
                    associations: vec![],
                };
                let mut child = Box::new(Engine::new(inner_def));
                child.variables = self.variables.clone();
                child.state = InstanceState::WaitingForTask;
                for inner_eid in inner_active {
                    child.tokens.push(Token {
                        element_id: inner_eid,
                    });
                }
                self.subprocess_stack.push(SubprocessFrame {
                    subprocess_element_id: esp.id.clone(),
                    child,
                });
                esp_ids_rebuilt.insert(esp.id.clone());
            }
        }

        self.state = if !self.tokens.is_empty() {
            InstanceState::WaitingForTask
        } else {
            InstanceState::Completed
        };
        Ok(self.result())
    }

    pub fn complete_task(
        &mut self,
        element_id: &str,
        output_variables: HashMap<String, Value>,
    ) -> Result<ExecutionResult, EngineError> {
        self.visited.clear();
        match &self.state {
            InstanceState::WaitingForTask => {}
            _ => return Err(EngineError::NoActiveTask),
        }
        self.record_visit(element_id, VisitEvent::Completed);

        // Delegate to subprocess child if element belongs there
        if let Some(frame) = self.subprocess_stack.last_mut() {
            if frame.child.has_element(element_id) {
                let child_result = frame.child.complete_task(element_id, output_variables)?;
                self.variables.extend(child_result.variables.clone());
                if child_result.is_completed {
                    let sp_elem_id = frame.subprocess_element_id.clone();
                    self.subprocess_stack.pop();
                    // Remove all tokens that were from the subprocess
                    self.tokens.retain(|t| t.element_id != element_id);
                    // Check if sp_elem_id is an EventSubProcess (no outgoing flow to follow)
                    let is_esp = self.definition.elements.iter().any(
                        |e| matches!(e, FlowElement::EventSubProcess(esp) if esp.id == sp_elem_id),
                    );
                    if is_esp {
                        // ESP completed — just update state based on remaining tokens
                        if self.tokens.is_empty() {
                            self.state = InstanceState::Completed;
                        } else {
                            self.state = InstanceState::WaitingForTask;
                        }
                    } else {
                        // Advance past the subprocess element in the parent
                        self.state = InstanceState::Running;
                        self.leave_and_advance(&sp_elem_id)?;
                        if !self.tokens.is_empty() && self.state == InstanceState::Running {
                            self.state = InstanceState::WaitingForTask;
                        }
                    }
                } else {
                    // Update tokens to reflect child's active elements
                    self.tokens.retain(|t| {
                        !frame_has_element_id(self.subprocess_stack.last(), &t.element_id)
                    });
                    for ae in &child_result.active_elements {
                        self.tokens.push(Token {
                            element_id: ae.element_id.clone(),
                        });
                    }
                    self.state = child_result.compat_state();
                }
                return Ok(self.result());
            }
        }

        // Check that a token exists at this element (non-subprocess path)
        let token_exists = self.tokens.iter().any(|t| t.element_id == element_id);
        if !token_exists {
            return Err(EngineError::WrongTask(element_id.to_string()));
        }

        self.variables.extend(output_variables);

        // Check if this is a multi-instance task with remaining iterations
        if let Some(&(idx, total)) = self.loop_state.get(element_id) {
            let next_idx = idx + 1;
            if next_idx < total {
                self.loop_state
                    .insert(element_id.to_string(), (next_idx, total));
                self.variables
                    .insert("__loop_index__".to_string(), serde_json::json!(next_idx));
                return Ok(self.result());
            } else {
                self.loop_state.remove(element_id);
                self.variables.remove("__loop_index__");
            }
        }

        // Remove the token for the completed element
        self.tokens.retain(|t| t.element_id != element_id);

        self.state = InstanceState::Running;
        self.leave_and_advance(element_id)?;

        if !self.tokens.is_empty() && self.state == InstanceState::Running {
            self.state = InstanceState::WaitingForTask;
        }

        Ok(self.result())
    }

    /// Returns the current loop index for the first active multi-instance element.
    pub fn current_loop_index(&self) -> Option<usize> {
        self.tokens
            .iter()
            .find_map(|t| self.loop_state.get(&t.element_id).map(|(idx, _)| *idx))
    }

    /// Returns true if this engine's definition contains an element with the given id.
    pub fn has_element(&self, id: &str) -> bool {
        self.definition.elements.iter().any(|e| e.id() == id)
    }

    fn advance_from(&mut self, element_id: &str) -> Result<(), EngineError> {
        let (element_type, outgoing_flow, timer_def, message_name, signal_ref, script_data) = {
            let element = self
                .find_element(element_id)
                .ok_or_else(|| EngineError::TargetNotFound(element_id.to_string()))?;
            let outgoing = element.outgoing().first().cloned();
            match element {
                FlowElement::StartEvent(_) | FlowElement::TimerStartEvent(_) => {
                    ("start", outgoing, None, None, None, None)
                }
                FlowElement::ServiceTask(_) => ("service", outgoing, None, None, None, None),
                FlowElement::EndEvent(_) => ("end", outgoing, None, None, None, None),
                FlowElement::ErrorEndEvent(ee) => (
                    "error_end",
                    outgoing,
                    None,
                    Some((ee.error_code.clone().unwrap_or_default(), None)),
                    None,
                    None,
                ),
                FlowElement::TerminateEndEvent(_) => {
                    ("terminate_end", outgoing, None, None, None, None)
                }
                FlowElement::ExclusiveGateway(_) => {
                    ("exclusive_gw", outgoing, None, None, None, None)
                }
                FlowElement::ParallelGateway(_) => {
                    ("parallel_gw", outgoing, None, None, None, None)
                }
                FlowElement::InclusiveGateway(_) => {
                    ("inclusive_gw", outgoing, None, None, None, None)
                }
                FlowElement::MultiInstanceTask(_) => {
                    ("multi_instance", outgoing, None, None, None, None)
                }
                FlowElement::SubProcess(_) => ("subprocess", outgoing, None, None, None, None),
                // BoundaryEvents are never advanced to directly; they're consulted on task failure
                FlowElement::BoundaryEvent(_) => ("boundary", outgoing, None, None, None, None),
                FlowElement::TimerIntermediateEvent(t) => {
                    ("timer", outgoing, t.timer.clone(), None, None, None)
                }
                FlowElement::MessageIntermediateCatchEvent(m) => (
                    "message_catch",
                    outgoing,
                    None,
                    Some((m.message_name.clone(), m.correlation_key.clone())),
                    None,
                    None,
                ),
                FlowElement::SignalIntermediateCatchEvent(s) => (
                    "signal_catch",
                    outgoing,
                    None,
                    None,
                    Some(s.signal_ref.clone()),
                    None,
                ),
                // Intermediate throw events pass through to the next element immediately.
                FlowElement::IntermediateThrowEvent(_) => {
                    ("throw", outgoing, None, None, None, None)
                }
                // MessageStartEvent: treated like a regular start — advance to next element.
                FlowElement::MessageStartEvent(_) => ("start", outgoing, None, None, None, None),
                // SignalStartEvent: treated like a regular start — advance to next element.
                FlowElement::SignalStartEvent(_) => ("start", outgoing, None, None, None, None),
                // MessageBoundaryEvent: never directly advanced to (consulted via receive_boundary_message).
                FlowElement::MessageBoundaryEvent(_) => {
                    ("boundary", outgoing, None, None, None, None)
                }
                // TimerBoundaryEvent: never directly advanced to (consulted via fire_boundary_timer).
                FlowElement::TimerBoundaryEvent(_) => {
                    ("boundary", outgoing, None, None, None, None)
                }
                // SignalBoundaryEvent: never directly advanced to (consulted via receive_boundary_signal).
                FlowElement::SignalBoundaryEvent(_) => {
                    ("boundary", outgoing, None, None, None, None)
                }
                // SignalIntermediateThrowEvent: pass-through (signal sending not yet implemented).
                FlowElement::SignalIntermediateThrowEvent(_) => {
                    ("throw", outgoing, None, None, None, None)
                }
                // SignalEndEvent: completes like end (signal sending not yet implemented).
                FlowElement::SignalEndEvent(_) => ("end", outgoing, None, None, None, None),
                // MessageEndEvent: emit message then complete (like end, but records thrown message).
                FlowElement::MessageEndEvent(me) => (
                    "message_end",
                    outgoing,
                    None,
                    Some((me.message_name.clone(), None)),
                    None,
                    None,
                ),
                // MessageIntermediateThrowEvent: emit message then pass through (like throw).
                FlowElement::MessageIntermediateThrowEvent(mte) => (
                    "message_throw",
                    outgoing,
                    None,
                    Some((mte.message_name.clone(), None)),
                    None,
                    None,
                ),
                // ReceiveTask: suspends and waits for a named message, like MessageIntermediateCatchEvent.
                FlowElement::ReceiveTask(r) => (
                    "message_catch",
                    outgoing,
                    None,
                    Some((r.message_name.clone(), r.correlation_key.clone())),
                    None,
                    None,
                ),
                FlowElement::EventBasedGateway(_) => {
                    ("event_based_gw", outgoing, None, None, None, None)
                }
                FlowElement::ScriptTask(st) => {
                    let script_data = Some((
                        st.script_format.clone(),
                        st.script.clone(),
                        st.result_variable.clone(),
                        st.id.clone(),
                    ));
                    ("script", outgoing, None, None, None, script_data)
                }
                // EscalationIntermediateThrowEvent: records escalation then passes through
                FlowElement::EscalationIntermediateThrowEvent(esc) => (
                    "escalation_throw",
                    outgoing,
                    None,
                    Some((esc.escalation_code.clone().unwrap_or_default(), None)),
                    None,
                    None,
                ),
                // EscalationEndEvent: records escalation then ends current path
                FlowElement::EscalationEndEvent(esc) => (
                    "escalation_end",
                    outgoing,
                    None,
                    Some((esc.escalation_code.clone().unwrap_or_default(), None)),
                    None,
                    None,
                ),
                // EscalationBoundaryEvent: never directly advanced to; consulted when subprocess throws escalation
                FlowElement::EscalationBoundaryEvent(_) => {
                    ("boundary", outgoing, None, None, None, None)
                }
                // LinkIntermediateThrowEvent: GOTO — jumps to matching LinkIntermediateCatchEvent by link_name
                FlowElement::LinkIntermediateThrowEvent(lt) => (
                    "link_throw",
                    outgoing,
                    None,
                    Some((lt.link_name.clone(), None)),
                    None,
                    None,
                ),
                // LinkIntermediateCatchEvent: target of a link throw — just passes through
                FlowElement::LinkIntermediateCatchEvent(_) => {
                    ("link_catch", outgoing, None, None, None, None)
                }
                // EventSubProcess: never directly advanced to; triggered by error/escalation/message/signal/timer
                FlowElement::EventSubProcess(_) => ("boundary", outgoing, None, None, None, None),
                // EventSubProcessStartEvent: lives inside EventSubProcess.inner_elements; never advanced to directly
                FlowElement::EventSubProcessStartEvent(_) => {
                    ("boundary", outgoing, None, None, None, None)
                }
            }
        };

        // Execute script if this is a ScriptTask
        if let Some((format, script, result_var, script_element_id)) = script_data {
            let evaluator = self.script_evaluators.get(&format).ok_or_else(|| {
                EngineError::UnsupportedScriptLanguage {
                    element_id: script_element_id.clone(),
                    language: format,
                }
            })?;
            let input = ScriptInput {
                script: &script,
                variables: &self.variables,
            };
            match evaluator.eval(input) {
                Ok(output) => {
                    if let (Some(name), Some(val)) = (result_var, output.return_value) {
                        self.variables.insert(name, val);
                    }
                    // TODO: scope merge (output.modified_variables) disabled until
                    // we define clear semantics for which script-local variables
                    // should propagate back into process variables.
                }
                Err(e) => {
                    return Err(EngineError::ScriptFailed {
                        element_id: script_element_id,
                        message: e.message,
                        line: e.line,
                    });
                }
            }
        }

        self.record_visit(element_id, VisitEvent::Activated);

        match element_type {
            "start" | "throw" | "script" => {
                self.record_visit(element_id, VisitEvent::Completed);
                let flow_id = outgoing_flow
                    .ok_or_else(|| EngineError::NoOutgoingFlow(element_id.to_string()))?
                    .clone();
                let next_id = self.flow_target(&flow_id)?;
                self.advance_from(&next_id)
            }
            "service" => {
                self.state = InstanceState::WaitingForTask;
                self.tokens.push(Token {
                    element_id: element_id.to_string(),
                });
                Ok(())
            }
            "end" => {
                self.record_visit(element_id, VisitEvent::Completed);
                if self.tokens.is_empty() {
                    self.state = InstanceState::Completed;
                }
                Ok(())
            }
            "error_end" => {
                self.record_visit(element_id, VisitEvent::ErrorThrown);
                // ErrorEndEvent: set the instance to Failed and record the thrown error code
                let error_code = message_name.map(|(code, _)| code).filter(|c| !c.is_empty());
                self.thrown_error = error_code;
                self.state = InstanceState::Failed;
                Ok(())
            }
            "terminate_end" => {
                self.record_visit(element_id, VisitEvent::Terminated);
                // TerminateEndEvent: kill all tokens and complete the instance immediately
                self.tokens.clear();
                self.state = InstanceState::Completed;
                Ok(())
            }
            "escalation_throw" => {
                self.record_visit(element_id, VisitEvent::EscalationThrown);
                // EscalationIntermediateThrowEvent: record escalation code, then pass through
                let escalation_code = message_name.map(|(code, _)| code).filter(|c| !c.is_empty());
                self.thrown_escalation = escalation_code.clone();
                self.leave_and_advance(element_id)?;
                // Check for escalation event subprocess
                if let Some((esp, is_interrupting)) =
                    self.find_escalation_event_subprocess(&escalation_code)
                {
                    self.trigger_esp_inline(&esp, HashMap::new(), is_interrupting)?;
                }
                Ok(())
            }
            "escalation_end" => {
                self.record_visit(element_id, VisitEvent::EscalationThrown);
                // EscalationEndEvent: record escalation code, then end current path
                let escalation_code = message_name.map(|(code, _)| code).filter(|c| !c.is_empty());
                self.thrown_escalation = escalation_code;
                if self.tokens.is_empty() {
                    self.state = InstanceState::Completed;
                }
                Ok(())
            }
            "link_throw" => {
                self.record_visit(element_id, VisitEvent::LinkJumped);
                // LinkIntermediateThrowEvent: find matching LinkIntermediateCatchEvent by link_name and jump
                let link_name = message_name.map(|(name, _)| name).unwrap_or_default();
                let catch_id = self
                    .definition
                    .elements
                    .iter()
                    .find_map(|e| match e {
                        FlowElement::LinkIntermediateCatchEvent(lc)
                            if lc.link_name == link_name =>
                        {
                            Some(lc.id.clone())
                        }
                        _ => None,
                    })
                    .ok_or_else(|| {
                        EngineError::TargetNotFound(format!(
                            "LinkIntermediateCatchEvent with name '{link_name}'"
                        ))
                    })?;
                self.advance_from(&catch_id)?;
                Ok(())
            }
            "link_catch" => {
                self.record_visit(element_id, VisitEvent::Completed);
                // LinkIntermediateCatchEvent: arrived via link throw — pass through to outgoing
                self.leave_and_advance(element_id)?;
                Ok(())
            }
            "message_end" => {
                self.record_visit(element_id, VisitEvent::MessageThrown);
                // Record the thrown message then behave like end
                if let Some((msg_name, _)) = message_name {
                    self.thrown_messages
                        .push((element_id.to_string(), msg_name));
                }
                if self.tokens.is_empty() {
                    self.state = InstanceState::Completed;
                }
                Ok(())
            }
            "message_throw" => {
                self.record_visit(element_id, VisitEvent::MessageThrown);
                // Record the thrown message then follow the outgoing flow (pass-through)
                if let Some((msg_name, _)) = message_name {
                    self.thrown_messages
                        .push((element_id.to_string(), msg_name));
                }
                let flow_id = outgoing_flow
                    .ok_or_else(|| EngineError::NoOutgoingFlow(element_id.to_string()))?
                    .clone();
                let next_id = self.flow_target(&flow_id)?;
                self.advance_from(&next_id)
            }
            "exclusive_gw" => {
                let (outgoing_flows, default_flow) = {
                    let element = self
                        .find_element(element_id)
                        .ok_or_else(|| EngineError::TargetNotFound(element_id.to_string()))?;
                    let outgoing = element.outgoing().to_vec();
                    let default_flow = if let FlowElement::ExclusiveGateway(gw) = element {
                        gw.default.clone()
                    } else {
                        None
                    };
                    (outgoing, default_flow)
                };

                let mut fallback: Option<String> = default_flow;

                for flow_id in &outgoing_flows {
                    let (has_condition, condition_matches) = {
                        let flow = self
                            .definition
                            .sequence_flows
                            .iter()
                            .find(|f| &f.id == flow_id)
                            .ok_or_else(|| EngineError::TargetNotFound(flow_id.clone()))?;
                        match &flow.condition_expression {
                            Some(expr) => (true, self.eval_condition(expr)),
                            None => (false, false),
                        }
                    };

                    if has_condition && condition_matches {
                        self.record_visit(element_id, VisitEvent::Completed);
                        let next = self.flow_target(flow_id)?;
                        return self.advance_from(&next);
                    } else if !has_condition && fallback.is_none() {
                        fallback = Some(flow_id.clone());
                    }
                }

                if let Some(fb_flow) = fallback {
                    self.record_visit(element_id, VisitEvent::Completed);
                    let next = self.flow_target(&fb_flow)?;
                    self.advance_from(&next)
                } else {
                    Err(EngineError::NoMatchingCondition(element_id.to_string()))
                }
            }
            "parallel_gw" => {
                let (incoming_count, outgoing_flows) = {
                    let el = self
                        .find_element(element_id)
                        .ok_or_else(|| EngineError::TargetNotFound(element_id.to_string()))?;
                    if let FlowElement::ParallelGateway(gw) = el {
                        (gw.incoming.len(), gw.outgoing.clone())
                    } else {
                        unreachable!()
                    }
                };

                if incoming_count > 1 {
                    let count = self.join_counts.entry(element_id.to_string()).or_insert(0);
                    *count += 1;
                    if *count < incoming_count {
                        return Ok(());
                    }
                    self.join_counts.remove(element_id);
                }

                self.record_visit(element_id, VisitEvent::Completed);
                for flow_id in outgoing_flows {
                    let next_id = self.flow_target(&flow_id)?;
                    self.advance_from(&next_id)?;
                }
                Ok(())
            }
            "inclusive_gw" => {
                let (incoming_len, outgoing_flows, default_flow) = {
                    let el = self
                        .find_element(element_id)
                        .ok_or_else(|| EngineError::TargetNotFound(element_id.to_string()))?;
                    if let FlowElement::InclusiveGateway(gw) = el {
                        (gw.incoming.len(), gw.outgoing.clone(), gw.default.clone())
                    } else {
                        unreachable!()
                    }
                };

                // --- JOIN logic (multiple incoming → wait for expected count) ---
                if incoming_len > 1 {
                    let expected = self
                        .inclusive_join_counts
                        .get(element_id)
                        .copied()
                        .unwrap_or(incoming_len); // fallback: treat as parallel join
                    let count = self.join_counts.entry(element_id.to_string()).or_insert(0);
                    *count += 1;
                    if *count < expected {
                        return Ok(());
                    }
                    self.join_counts.remove(element_id);
                    self.inclusive_join_counts.remove(element_id);
                    self.record_visit(element_id, VisitEvent::Completed);
                    for flow_id in outgoing_flows {
                        let next_id = self.flow_target(&flow_id)?;
                        self.advance_from(&next_id)?;
                    }
                    return Ok(());
                }

                // --- FORK logic (1 incoming → evaluate conditions, take ALL matching) ---
                let mut activated_flows: Vec<String> = Vec::new();

                for flow_id in &outgoing_flows {
                    // Skip the default flow on first pass
                    if Some(flow_id.as_str()) == default_flow.as_deref() {
                        continue;
                    }
                    let has_condition_and_matches = {
                        let flow = self
                            .definition
                            .sequence_flows
                            .iter()
                            .find(|f| &f.id == flow_id)
                            .ok_or_else(|| EngineError::TargetNotFound(flow_id.clone()))?;
                        match &flow.condition_expression {
                            Some(expr) => (true, self.eval_condition(expr)),
                            None => (false, false),
                        }
                    };
                    match has_condition_and_matches {
                        (true, true) => activated_flows.push(flow_id.clone()),
                        (false, _) => activated_flows.push(flow_id.clone()), // unconditional
                        _ => {}
                    }
                }

                // If no conditional/unconditional flows matched, take default
                if activated_flows.is_empty() {
                    match &default_flow {
                        Some(df) => activated_flows.push(df.clone()),
                        None => {
                            return Err(EngineError::NoMatchingCondition(element_id.to_string()))
                        }
                    }
                }

                let activated_count = activated_flows.len();

                // Store expected count for downstream join gateways via 2-hop trace:
                // fork → (flow) → task → (flow) → join
                // Collect join IDs first to avoid borrow conflict with self.find_element
                let mut join_updates: Vec<String> = Vec::new();
                for flow_id in &activated_flows {
                    let task_id = self.flow_target(flow_id)?;
                    let task_outgoing: Vec<String> = self
                        .find_element(&task_id)
                        .map(|el| el.outgoing().to_vec())
                        .unwrap_or_default();
                    for task_out_flow in &task_outgoing {
                        let maybe_join = self.flow_target(task_out_flow)?;
                        let is_inclusive_join = self.find_element(&maybe_join)
                            .map(|el| matches!(el, FlowElement::InclusiveGateway(gw) if gw.incoming.len() > 1))
                            .unwrap_or(false);
                        if is_inclusive_join {
                            join_updates.push(maybe_join);
                        }
                    }
                }
                for join_id in join_updates {
                    self.inclusive_join_counts.insert(join_id, activated_count);
                }

                // Advance from all activated targets
                self.record_visit(element_id, VisitEvent::Completed);
                for flow_id in activated_flows {
                    let next_id = self.flow_target(&flow_id)?;
                    self.advance_from(&next_id)?;
                }
                Ok(())
            }
            "event_based_gw" => {
                let outgoing_flows = {
                    let el = self
                        .find_element(element_id)
                        .ok_or_else(|| EngineError::TargetNotFound(element_id.to_string()))?;
                    if let FlowElement::EventBasedGateway(gw) = el {
                        gw.outgoing.clone()
                    } else {
                        unreachable!()
                    }
                };
                // Fork to all catch event targets — XOR cancellation is handled server-side
                self.record_visit(element_id, VisitEvent::Completed);
                for flow_id in outgoing_flows {
                    let next_id = self.flow_target(&flow_id)?;
                    self.advance_from(&next_id)?;
                }
                Ok(())
            }
            "multi_instance" => {
                let (input_ref, _is_sequential) = {
                    let el = self
                        .find_element(element_id)
                        .ok_or_else(|| EngineError::TargetNotFound(element_id.to_string()))?;
                    if let FlowElement::MultiInstanceTask(mi) = el {
                        (mi.loop_data_input_ref.clone(), mi.is_sequential)
                    } else {
                        unreachable!()
                    }
                };

                let total = self
                    .variables
                    .get(&input_ref)
                    .and_then(|v| v.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);

                if total == 0 {
                    return self.leave_and_advance(element_id);
                }

                self.loop_state.insert(element_id.to_string(), (0, total));
                self.variables
                    .insert("__loop_index__".to_string(), serde_json::json!(0));
                self.state = InstanceState::WaitingForTask;
                self.tokens.push(Token {
                    element_id: element_id.to_string(),
                });
                Ok(())
            }
            "subprocess" => {
                let (inner_elements, inner_flows) = {
                    let el = self
                        .find_element(element_id)
                        .ok_or_else(|| EngineError::TargetNotFound(element_id.to_string()))?;
                    if let FlowElement::SubProcess(sp) = el {
                        (sp.inner_elements.clone(), sp.inner_flows.clone())
                    } else {
                        unreachable!()
                    }
                };

                let inner_def = ProcessDefinition {
                    id: format!("{element_id}__inner"),
                    name: None,
                    elements: inner_elements,
                    sequence_flows: inner_flows,
                    annotations: vec![],
                    associations: vec![],
                };

                let mut child = Box::new(Engine::new(inner_def));
                let child_result = child.start(self.variables.clone())?;
                self.variables.extend(child_result.variables.clone());

                if child_result.is_completed {
                    // Check for escalation boundary events before advancing
                    if let Some(ref esc_code) = child_result.thrown_escalation {
                        if let Some((boundary_outgoing, is_interrupting)) =
                            self.find_escalation_boundary(element_id, &Some(esc_code.clone()))
                        {
                            let next_id = self.flow_target(&boundary_outgoing)?;
                            if is_interrupting {
                                // Interrupting: cancel subprocess, route to boundary path
                                self.advance_from(&next_id)?;
                            } else {
                                // Non-interrupting: keep subprocess completion, spawn parallel path
                                self.leave_and_advance(element_id)?;
                                self.advance_from(&next_id)?;
                            }
                        } else if let Some((esp, is_interrupting)) =
                            self.find_escalation_event_subprocess(&Some(esc_code.clone()))
                        {
                            // Escalation event subprocess in parent scope
                            if is_interrupting {
                                // Interrupting: don't advance normally, ESP takes over
                                self.trigger_esp_inline(&esp, HashMap::new(), true)?;
                            } else {
                                // Non-interrupting: advance normally AND trigger ESP
                                self.leave_and_advance(element_id)?;
                                self.trigger_esp_inline(&esp, HashMap::new(), false)?;
                            }
                        } else {
                            // No matching escalation boundary or ESP — just continue normally
                            self.leave_and_advance(element_id)?;
                        }
                    } else {
                        // No escalation — advance in parent normally
                        self.leave_and_advance(element_id)?;
                    }
                } else if child_result.is_failed {
                    // Subprocess failed — check for error boundary events
                    let thrown_code = child_result.thrown_error.clone();
                    // Find matching error boundary: first try specific code match, then catch-all
                    let matching_boundary = self.find_error_boundary(element_id, &thrown_code);
                    if let Some(boundary) = matching_boundary {
                        let flow_id = boundary
                            .outgoing
                            .first()
                            .ok_or_else(|| EngineError::NoOutgoingFlow(boundary.id.clone()))?
                            .clone();
                        let next_id = self.flow_target(&flow_id)?;
                        self.state = InstanceState::Running;
                        self.advance_from(&next_id)?;
                    } else {
                        // No matching boundary — propagate failure
                        self.state = InstanceState::Failed;
                        self.thrown_error = thrown_code;
                    }
                } else {
                    // Subprocess is waiting — add tokens for active inner elements
                    for ae in &child_result.active_elements {
                        self.tokens.push(Token {
                            element_id: ae.element_id.clone(),
                        });
                    }
                    self.state = child_result.compat_state();
                    self.subprocess_stack.push(SubprocessFrame {
                        subprocess_element_id: element_id.to_string(),
                        child,
                    });
                }
                Ok(())
            }
            "timer" => {
                let definition = timer_def.unwrap_or_else(TimerDefinition::zero_duration);
                self.state = InstanceState::WaitingForTimer {
                    element_id: element_id.to_string(),
                    definition,
                };
                self.tokens.push(Token {
                    element_id: element_id.to_string(),
                });
                Ok(())
            }
            "message_catch" => {
                let (mname, ck_expr) = message_name.unwrap_or_else(|| (String::new(), None));
                self.state = InstanceState::WaitingForMessage {
                    element_id: element_id.to_string(),
                    message_name: mname,
                    correlation_key_expr: ck_expr,
                };
                self.tokens.push(Token {
                    element_id: element_id.to_string(),
                });
                Ok(())
            }
            "signal_catch" => {
                let sref = signal_ref.unwrap_or_default().clone();
                self.state = InstanceState::WaitingForSignal {
                    element_id: element_id.to_string(),
                    signal_ref: sref,
                };
                self.tokens.push(Token {
                    element_id: element_id.to_string(),
                });
                Ok(())
            }
            "boundary" => {
                // BoundaryEvents are never directly advanced to
                Err(EngineError::TargetNotFound(element_id.to_string()))
            }
            _ => unreachable!(),
        }
    }

    /// Called when a task fails with no retries remaining.
    /// If an error boundary event is attached, routes to it instead of failing the instance.
    /// If an error event subprocess matches, triggers it instead.
    /// Returns Ok(result) if boundary/ESP caught it, Err if no boundary (caller marks instance FAILED).
    pub fn fail_task(
        &mut self,
        element_id: &str,
        error_code: Option<String>,
    ) -> Result<ExecutionResult, EngineError> {
        self.visited.clear();
        self.record_visit(element_id, VisitEvent::ErrorThrown);
        // Remove token for this element
        self.tokens.retain(|t| t.element_id != element_id);

        // Find a BoundaryEvent attached to this element
        let boundary = self
            .definition
            .elements
            .iter()
            .find(|e| {
                if let FlowElement::BoundaryEvent(b) = e {
                    b.attached_to_ref == element_id
                } else {
                    false
                }
            })
            .cloned();

        if let Some(FlowElement::BoundaryEvent(b)) = boundary {
            let flow_id = b
                .outgoing
                .first()
                .ok_or_else(|| EngineError::NoOutgoingFlow(b.id.clone()))?
                .clone();
            let next_id = self.flow_target(&flow_id)?;
            self.state = InstanceState::Running;
            self.advance_from(&next_id)?;
            Ok(self.result())
        } else if let Some((esp, is_interrupting)) = self.find_error_event_subprocess(&error_code) {
            self.state = InstanceState::Running;
            self.trigger_esp_inline(&esp, HashMap::new(), is_interrupting)?;
            Ok(self.result())
        } else {
            // No boundary or ESP — propagate failure
            self.state = InstanceState::Failed;
            Ok(self.result())
        }
    }

    /// Find a matching error boundary event attached to the given element.
    /// Priority: specific error code match first, then catch-all (no error code).
    fn find_error_boundary(
        &self,
        attached_to: &str,
        thrown_code: &Option<String>,
    ) -> Option<BoundaryEvent> {
        let boundaries: Vec<&BoundaryEvent> = self
            .definition
            .elements
            .iter()
            .filter_map(|e| match e {
                FlowElement::BoundaryEvent(b)
                    if b.attached_to_ref == attached_to && b.event_type == "error" =>
                {
                    Some(b)
                }
                _ => None,
            })
            .collect();

        // First try specific match
        if let Some(ref code) = thrown_code {
            if let Some(b) = boundaries
                .iter()
                .find(|b| b.error_code.as_deref() == Some(code))
            {
                return Some((*b).clone());
            }
        }

        // Then try catch-all (no error code)
        boundaries
            .iter()
            .find(|b| b.error_code.is_none())
            .map(|b| (*b).clone())
    }

    /// Find a matching escalation boundary event attached to the given element.
    /// Returns (first_outgoing_flow_id, is_interrupting) if found.
    /// Priority: specific escalation code match first, then catch-all (no escalation code).
    fn find_escalation_boundary(
        &self,
        attached_to: &str,
        thrown_code: &Option<String>,
    ) -> Option<(String, bool)> {
        let boundaries: Vec<&EscalationBoundaryEvent> = self
            .definition
            .elements
            .iter()
            .filter_map(|e| match e {
                FlowElement::EscalationBoundaryEvent(b) if b.attached_to_ref == attached_to => {
                    Some(b)
                }
                _ => None,
            })
            .collect();

        // First try specific match
        if let Some(ref code) = thrown_code {
            if let Some(b) = boundaries
                .iter()
                .find(|b| b.escalation_code.as_deref() == Some(code.as_str()))
            {
                let flow = b.outgoing.first()?.clone();
                return Some((flow, b.is_interrupting));
            }
        }

        // Then try catch-all (no escalation code)
        boundaries
            .iter()
            .find(|b| b.escalation_code.is_none())
            .and_then(|b| {
                let flow = b.outgoing.first()?.clone();
                Some((flow, b.is_interrupting))
            })
    }

    /// Find an error event subprocess matching the thrown error code.
    /// Priority: specific code match first, then catch-all.
    fn find_error_event_subprocess(
        &self,
        thrown_code: &Option<String>,
    ) -> Option<(EventSubProcess, bool)> {
        let mut specific: Option<(EventSubProcess, bool)> = None;
        let mut catchall: Option<(EventSubProcess, bool)> = None;
        for elem in &self.definition.elements {
            if let FlowElement::EventSubProcess(esp) = elem {
                if let Some(start) = esp.inner_elements.iter().find_map(|e| match e {
                    FlowElement::EventSubProcessStartEvent(s) => Some(s),
                    _ => None,
                }) {
                    if let EventSubProcessTrigger::Error { error_code } = &start.trigger {
                        if error_code.is_none() {
                            catchall = Some((esp.clone(), start.is_interrupting));
                        } else if let (Some(thrown), Some(expected)) = (thrown_code, error_code) {
                            if thrown == expected {
                                specific = Some((esp.clone(), start.is_interrupting));
                            }
                        }
                    }
                }
            }
        }
        specific.or(catchall)
    }

    /// Find an escalation event subprocess matching the thrown escalation code.
    /// Priority: specific code match first, then catch-all.
    fn find_escalation_event_subprocess(
        &self,
        thrown_code: &Option<String>,
    ) -> Option<(EventSubProcess, bool)> {
        let mut specific: Option<(EventSubProcess, bool)> = None;
        let mut catchall: Option<(EventSubProcess, bool)> = None;
        for elem in &self.definition.elements {
            if let FlowElement::EventSubProcess(esp) = elem {
                if let Some(start) = esp.inner_elements.iter().find_map(|e| match e {
                    FlowElement::EventSubProcessStartEvent(s) => Some(s),
                    _ => None,
                }) {
                    if let EventSubProcessTrigger::Escalation { escalation_code } = &start.trigger {
                        if escalation_code.is_none() {
                            catchall = Some((esp.clone(), start.is_interrupting));
                        } else if let (Some(thrown), Some(expected)) =
                            (thrown_code, escalation_code)
                        {
                            if thrown == expected {
                                specific = Some((esp.clone(), start.is_interrupting));
                            }
                        }
                    }
                }
            }
        }
        specific.or(catchall)
    }

    /// Trigger an event subprocess inline.
    /// If interrupting, clears all current tokens (cancels parent flow).
    /// Runs ESP child from its start event's first outgoing flow.
    fn trigger_esp_inline(
        &mut self,
        esp: &EventSubProcess,
        vars: HashMap<String, Value>,
        is_interrupting: bool,
    ) -> Result<(), EngineError> {
        if is_interrupting {
            self.tokens.clear();
        }
        self.variables.extend(vars);

        let inner_def = crate::model::ProcessDefinition {
            id: format!("{}_esp", esp.id),
            name: None,
            elements: esp.inner_elements.clone(),
            sequence_flows: esp.inner_flows.clone(),
            annotations: vec![],
            associations: vec![],
        };

        // Find the ESP start event and its first outgoing flow
        let (start_event_id, flow_id) = inner_def
            .elements
            .iter()
            .find_map(|e| match e {
                FlowElement::EventSubProcessStartEvent(s) => {
                    let flow = s.outgoing.first()?.clone();
                    Some((s.id.clone(), flow))
                }
                _ => None,
            })
            .ok_or_else(|| EngineError::TargetNotFound(esp.id.clone()))?;

        let mut child = Box::new(Engine::new(inner_def));
        child.variables = self.variables.clone();
        child.state = InstanceState::Running;
        child.record_visit(&start_event_id, VisitEvent::Activated);
        child.record_visit(&start_event_id, VisitEvent::Completed);

        let next_id = child.flow_target(&flow_id)?;
        child.advance_from(&next_id)?;

        let child_result = child.result();
        if child_result.is_completed {
            self.variables.extend(child_result.variables);
            if self.tokens.is_empty() {
                self.state = InstanceState::Completed;
            }
        } else {
            self.variables.extend(child_result.variables.clone());
            for ae in &child_result.active_elements {
                self.tokens.push(Token {
                    element_id: ae.element_id.clone(),
                });
            }
            if !self.tokens.is_empty() {
                self.state = InstanceState::WaitingForTask;
            }
            self.subprocess_stack.push(SubprocessFrame {
                subprocess_element_id: esp.id.clone(),
                child,
            });
        }

        Ok(())
    }

    /// Triggered by server when an external event (message/signal/timer) fires an event subprocess.
    pub fn trigger_event_subprocess(
        &mut self,
        esp_id: &str,
        vars: HashMap<String, Value>,
    ) -> Result<ExecutionResult, EngineError> {
        self.visited.clear();
        let esp = self
            .definition
            .elements
            .iter()
            .find_map(|e| match e {
                FlowElement::EventSubProcess(esp) if esp.id == esp_id => Some(esp.clone()),
                _ => None,
            })
            .ok_or_else(|| EngineError::TargetNotFound(esp_id.to_string()))?;

        let is_interrupting = esp
            .inner_elements
            .iter()
            .find_map(|e| match e {
                FlowElement::EventSubProcessStartEvent(s) => Some(s.is_interrupting),
                _ => None,
            })
            .unwrap_or(true);

        self.state = InstanceState::Running;
        self.trigger_esp_inline(&esp, vars, is_interrupting)?;
        Ok(self.result())
    }

    fn record_visit(&mut self, element_id: &str, event: VisitEvent) {
        let (element_name, element_type) = self
            .find_element(element_id)
            .map(|e| (e.name().map(|s| s.to_string()), e.type_name().to_string()))
            .unwrap_or((None, "Unknown".to_string()));
        self.visited.push(VisitedElement {
            element_id: element_id.to_string(),
            element_name,
            element_type,
            event,
        });
    }

    fn leave_and_advance(&mut self, element_id: &str) -> Result<(), EngineError> {
        let flow_id = {
            let element = self
                .find_element(element_id)
                .ok_or_else(|| EngineError::TargetNotFound(element_id.to_string()))?;
            element
                .outgoing()
                .first()
                .ok_or_else(|| EngineError::NoOutgoingFlow(element_id.to_string()))?
                .clone()
        };
        let next_id = self.flow_target(&flow_id)?;
        self.advance_from(&next_id)
    }

    pub fn fire_timer(&mut self, element_id: &str) -> Result<ExecutionResult, EngineError> {
        self.visited.clear();
        // Check by token presence rather than by engine state: the state after engine.resume()
        // is WaitingForTask, not WaitingForTimer, even when the active element is a timer.
        if !self.tokens.iter().any(|t| t.element_id == element_id) {
            return Err(EngineError::NoActiveTask);
        }
        self.record_visit(element_id, VisitEvent::Completed);
        self.tokens.retain(|t| t.element_id != element_id);
        self.state = InstanceState::Running;
        self.leave_and_advance(element_id)?;
        Ok(self.result())
    }

    pub fn receive_message(
        &mut self,
        element_id: &str,
        message_variables: HashMap<String, Value>,
    ) -> Result<ExecutionResult, EngineError> {
        self.visited.clear();
        // Check by token presence rather than by engine state: the state after engine.resume()
        // is WaitingForTask, not WaitingForMessage, even when the active element is a message catch event.
        if !self.tokens.iter().any(|t| t.element_id == element_id) {
            return Err(EngineError::NoActiveTask);
        }
        self.record_visit(element_id, VisitEvent::Completed);
        self.variables.extend(message_variables);
        self.tokens.retain(|t| t.element_id != element_id);
        self.state = InstanceState::Running;
        self.leave_and_advance(element_id)?;
        Ok(self.result())
    }

    /// Deliver a message to a MessageBoundaryEvent attached to an active task.
    /// If interrupting, the attached task token is removed.
    /// If non-interrupting, the task token is kept and a parallel path is started.
    pub fn receive_boundary_message(
        &mut self,
        boundary_element_id: &str,
        message_variables: HashMap<String, Value>,
    ) -> Result<ExecutionResult, EngineError> {
        self.visited.clear();
        let (attached_to, is_interrupting, outgoing_flow) = {
            let element = self
                .definition
                .elements
                .iter()
                .find(|e| e.id() == boundary_element_id)
                .ok_or_else(|| EngineError::TargetNotFound(boundary_element_id.to_string()))?;
            match element {
                crate::model::FlowElement::MessageBoundaryEvent(mb) => {
                    let flow = mb
                        .outgoing
                        .first()
                        .ok_or_else(|| {
                            EngineError::NoOutgoingFlow(boundary_element_id.to_string())
                        })?
                        .clone();
                    (mb.attached_to_ref.clone(), mb.is_interrupting, flow)
                }
                _ => return Err(EngineError::TargetNotFound(boundary_element_id.to_string())),
            }
        };

        self.record_visit(boundary_element_id, VisitEvent::Activated);
        self.variables.extend(message_variables);

        if is_interrupting {
            // Cancel the active task token
            self.tokens.retain(|t| t.element_id != attached_to);
        }

        self.state = InstanceState::Running;
        let next_id = self.flow_target(&outgoing_flow)?;
        self.advance_from(&next_id)?;

        if !self.tokens.is_empty() && self.state == InstanceState::Running {
            self.state = InstanceState::WaitingForTask;
        }

        Ok(self.result())
    }

    pub fn fire_boundary_timer(
        &mut self,
        boundary_element_id: &str,
    ) -> Result<ExecutionResult, EngineError> {
        self.visited.clear();
        let (attached_to, is_interrupting, outgoing_flow) = {
            let element = self
                .definition
                .elements
                .iter()
                .find(|e| e.id() == boundary_element_id)
                .ok_or_else(|| EngineError::TargetNotFound(boundary_element_id.to_string()))?;
            match element {
                crate::model::FlowElement::TimerBoundaryEvent(tb) => {
                    let flow = tb
                        .outgoing
                        .first()
                        .ok_or_else(|| {
                            EngineError::NoOutgoingFlow(boundary_element_id.to_string())
                        })?
                        .clone();
                    (tb.attached_to_ref.clone(), tb.is_interrupting, flow)
                }
                _ => return Err(EngineError::TargetNotFound(boundary_element_id.to_string())),
            }
        };

        self.record_visit(boundary_element_id, VisitEvent::Activated);

        if is_interrupting {
            self.tokens.retain(|t| t.element_id != attached_to);
        }

        self.state = InstanceState::Running;
        let next_id = self.flow_target(&outgoing_flow)?;
        self.advance_from(&next_id)?;

        if !self.tokens.is_empty() && self.state == InstanceState::Running {
            self.state = InstanceState::WaitingForTask;
        }

        Ok(self.result())
    }

    /// Fire a signal boundary event attached to a task.
    /// If interrupting, cancels the task token; if non-interrupting, keeps it.
    pub fn receive_boundary_signal(
        &mut self,
        boundary_element_id: &str,
        signal_variables: HashMap<String, Value>,
    ) -> Result<ExecutionResult, EngineError> {
        self.visited.clear();
        self.variables.extend(signal_variables);

        let (attached_to, is_interrupting, outgoing_flow) = {
            let element = self
                .definition
                .elements
                .iter()
                .find(|e| e.id() == boundary_element_id)
                .ok_or_else(|| EngineError::TargetNotFound(boundary_element_id.to_string()))?;
            match element {
                crate::model::FlowElement::SignalBoundaryEvent(sb) => {
                    let flow = sb
                        .outgoing
                        .first()
                        .ok_or_else(|| {
                            EngineError::NoOutgoingFlow(boundary_element_id.to_string())
                        })?
                        .clone();
                    (sb.attached_to_ref.clone(), sb.is_interrupting, flow)
                }
                _ => return Err(EngineError::TargetNotFound(boundary_element_id.to_string())),
            }
        };

        self.record_visit(boundary_element_id, VisitEvent::Activated);

        if is_interrupting {
            self.tokens.retain(|t| t.element_id != attached_to);
        }

        self.state = InstanceState::Running;
        let next_id = self.flow_target(&outgoing_flow)?;
        self.advance_from(&next_id)?;

        if !self.tokens.is_empty() && self.state == InstanceState::Running {
            self.state = InstanceState::WaitingForTask;
        }

        Ok(self.result())
    }

    pub fn receive_signal(
        &mut self,
        element_id: &str,
        signal_variables: HashMap<String, Value>,
    ) -> Result<ExecutionResult, EngineError> {
        self.visited.clear();
        // Check by token presence rather than by engine state: the state after engine.resume()
        // is WaitingForTask, not WaitingForSignal, even when the active element is a signal catch event.
        if !self.tokens.iter().any(|t| t.element_id == element_id) {
            return Err(EngineError::NoActiveTask);
        }
        self.record_visit(element_id, VisitEvent::Completed);
        self.variables.extend(signal_variables);
        self.tokens.retain(|t| t.element_id != element_id);
        self.state = InstanceState::Running;
        self.leave_and_advance(element_id)?;
        Ok(self.result())
    }

    pub fn join_counts(&self) -> &HashMap<String, usize> {
        &self.join_counts
    }

    pub fn loop_state(&self) -> &HashMap<String, (usize, usize)> {
        &self.loop_state
    }

    pub fn inclusive_join_counts(&self) -> &HashMap<String, usize> {
        &self.inclusive_join_counts
    }

    fn find_element(&self, id: &str) -> Option<&FlowElement> {
        self.definition.elements.iter().find(|e| e.id() == id)
    }

    fn flow_target(&self, flow_id: &str) -> Result<String, EngineError> {
        self.definition
            .sequence_flows
            .iter()
            .find(|f| f.id == flow_id)
            .map(|f| f.target_ref.clone())
            .ok_or_else(|| EngineError::TargetNotFound(flow_id.to_string()))
    }

    fn element_wait_state(&self, element_id: &str) -> WaitState {
        let el = self
            .definition
            .elements
            .iter()
            .find(|e| e.id() == element_id)
            .or_else(|| {
                self.subprocess_stack.iter().rev().find_map(|frame| {
                    frame
                        .child
                        .definition
                        .elements
                        .iter()
                        .find(|e| e.id() == element_id)
                })
            });
        match el {
            Some(FlowElement::ServiceTask(t)) => WaitState::Task {
                topic: t.topic.clone(),
            },
            Some(FlowElement::MultiInstanceTask(_)) => WaitState::Task { topic: None },
            Some(FlowElement::ReceiveTask(t)) => WaitState::Message {
                message_name: t.message_name.clone(),
                correlation_key_expr: t.correlation_key.clone(),
            },
            Some(FlowElement::TimerIntermediateEvent(t)) => WaitState::Timer {
                definition: t
                    .timer
                    .clone()
                    .unwrap_or_else(TimerDefinition::zero_duration),
            },
            Some(FlowElement::MessageIntermediateCatchEvent(t)) => WaitState::Message {
                message_name: t.message_name.clone(),
                correlation_key_expr: t.correlation_key.clone(),
            },
            Some(FlowElement::SignalIntermediateCatchEvent(t)) => WaitState::Signal {
                signal_ref: t.signal_ref.clone(),
            },
            _ => WaitState::Task { topic: None }, // fallback for subprocess active elements
        }
    }

    fn collect_esp_subscriptions(&self) -> Vec<EventSubProcessSubscription> {
        let mut subs = Vec::new();

        // ESPs currently active (already triggered, child running in subprocess_stack).
        // These should NOT emit subscriptions — they are already firing.
        let active_esp_ids: std::collections::HashSet<&str> = self
            .subprocess_stack
            .iter()
            .map(|frame| frame.subprocess_element_id.as_str())
            .collect();

        // Root scope event subprocesses
        for elem in &self.definition.elements {
            if let FlowElement::EventSubProcess(esp) = elem {
                if active_esp_ids.contains(esp.id.as_str()) {
                    continue; // already running — don't re-subscribe
                }
                if let Some(sub) = Self::esp_to_subscription(esp, None) {
                    subs.push(sub);
                }
            }
        }

        // Active subprocess frames (nested scopes)
        for frame in &self.subprocess_stack {
            let scope_id = Some(frame.subprocess_element_id.clone());
            let child_active_esp_ids: std::collections::HashSet<&str> = frame
                .child
                .subprocess_stack
                .iter()
                .map(|f| f.subprocess_element_id.as_str())
                .collect();
            for elem in &frame.child.definition.elements {
                if let FlowElement::EventSubProcess(esp) = elem {
                    if child_active_esp_ids.contains(esp.id.as_str()) {
                        continue;
                    }
                    if let Some(sub) = Self::esp_to_subscription(esp, scope_id.clone()) {
                        subs.push(sub);
                    }
                }
            }
        }

        subs
    }

    fn esp_to_subscription(
        esp: &EventSubProcess,
        scope_id: Option<String>,
    ) -> Option<EventSubProcessSubscription> {
        use crate::model::EventSubProcessSubscription;
        let start = esp.inner_elements.iter().find_map(|e| match e {
            FlowElement::EventSubProcessStartEvent(s) => Some(s),
            _ => None,
        })?;

        // Only external triggers go into subscriptions; error/escalation handled synchronously
        match &start.trigger {
            EventSubProcessTrigger::Error { .. } | EventSubProcessTrigger::Escalation { .. } => {
                None
            }
            trigger => Some(EventSubProcessSubscription {
                esp_id: esp.id.clone(),
                trigger: trigger.clone(),
                scope_id,
                is_interrupting: start.is_interrupting,
            }),
        }
    }

    fn result(&mut self) -> ExecutionResult {
        let is_completed = self.state == InstanceState::Completed;
        let is_failed = self.state == InstanceState::Failed;
        let active_elements = self
            .tokens
            .iter()
            .map(|t| ActiveElement {
                element_id: t.element_id.clone(),
                wait_state: self.element_wait_state(&t.element_id),
            })
            .collect();
        let event_subprocess_subscriptions = if !is_completed && !is_failed {
            self.collect_esp_subscriptions()
        } else {
            Vec::new()
        };
        ExecutionResult {
            active_elements,
            variables: self.variables.clone(),
            thrown_messages: self.thrown_messages.clone(),
            thrown_error: self.thrown_error.clone(),
            thrown_escalation: self.thrown_escalation.clone(),
            visited: std::mem::take(&mut self.visited),
            event_subprocess_subscriptions,
            is_completed,
            is_failed,
        }
    }

    fn eval_condition(&self, expr: &str) -> bool {
        expression::eval(expr, &self.variables)
    }
}

fn frame_has_element_id(frame: Option<&SubprocessFrame>, id: &str) -> bool {
    frame.map(|f| f.child.has_element(id)).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_bpmn;

    #[test]
    fn engine_result_exposes_single_active_element_for_service_task() {
        let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="t1"/>
    <serviceTask id="t1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="t1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let mut engine = Engine::new(def);
        let result = engine.start(Default::default()).unwrap();
        assert_eq!(result.compat_state(), InstanceState::WaitingForTask);
        assert_eq!(result.active_element_ids(), vec!["t1".to_string()]);
    }

    const FORK_JOIN_BPMN: &str = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f0</outgoing></startEvent>
    <sequenceFlow id="f0" sourceRef="s" targetRef="fork"/>
    <parallelGateway id="fork">
      <incoming>f0</incoming>
      <outgoing>fa</outgoing>
      <outgoing>fb</outgoing>
    </parallelGateway>
    <sequenceFlow id="fa" sourceRef="fork" targetRef="ta"/>
    <sequenceFlow id="fb" sourceRef="fork" targetRef="tb"/>
    <serviceTask id="ta"><incoming>fa</incoming><outgoing>fca</outgoing></serviceTask>
    <serviceTask id="tb"><incoming>fb</incoming><outgoing>fcb</outgoing></serviceTask>
    <sequenceFlow id="fca" sourceRef="ta" targetRef="join"/>
    <sequenceFlow id="fcb" sourceRef="tb" targetRef="join"/>
    <parallelGateway id="join">
      <incoming>fca</incoming>
      <incoming>fcb</incoming>
      <outgoing>fe</outgoing>
    </parallelGateway>
    <sequenceFlow id="fe" sourceRef="join" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;

    #[test]
    fn fork_creates_two_tokens() {
        let def = parse_bpmn(FORK_JOIN_BPMN).unwrap();
        let mut engine = Engine::new(def);
        let result = engine.start(Default::default()).unwrap();
        assert_eq!(result.compat_state(), InstanceState::WaitingForTask);
        let mut active = result.active_element_ids();
        active.sort();
        assert_eq!(active, vec!["ta".to_string(), "tb".to_string()]);
    }

    #[test]
    fn join_waits_until_both_branches_complete() {
        let def = parse_bpmn(FORK_JOIN_BPMN).unwrap();
        let mut engine = Engine::new(def);
        engine.start(Default::default()).unwrap();

        let r1 = engine.complete_task("ta", Default::default()).unwrap();
        assert_eq!(r1.compat_state(), InstanceState::WaitingForTask);
        assert!(r1.active_element_ids().contains(&"tb".to_string()));

        let r2 = engine.complete_task("tb", Default::default()).unwrap();
        assert_eq!(r2.compat_state(), InstanceState::Completed);
        assert!(r2.active_element_ids().is_empty());
    }

    #[test]
    fn multi_instance_sequential_runs_once_per_item() {
        let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="mi_task"/>
    <serviceTask id="mi_task">
      <multiInstanceLoopCharacteristics isSequential="true">
        <loopDataInputRef>items</loopDataInputRef>
      </multiInstanceLoopCharacteristics>
      <outgoing>f2</outgoing>
    </serviceTask>
    <sequenceFlow id="f2" sourceRef="mi_task" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let items = serde_json::json!(["a", "b", "c"]);
        let mut vars = HashMap::new();
        vars.insert("items".to_string(), items);
        let mut engine = Engine::new(def);
        let r = engine.start(vars).unwrap();
        assert_eq!(r.compat_state(), InstanceState::WaitingForTask);
        assert_eq!(engine.current_loop_index(), Some(0));

        let r2 = engine.complete_task("mi_task", Default::default()).unwrap();
        assert_eq!(r2.compat_state(), InstanceState::WaitingForTask);
        assert_eq!(engine.current_loop_index(), Some(1));

        let r3 = engine.complete_task("mi_task", Default::default()).unwrap();
        assert_eq!(r3.compat_state(), InstanceState::WaitingForTask);
        assert_eq!(engine.current_loop_index(), Some(2));

        let r4 = engine.complete_task("mi_task", Default::default()).unwrap();
        assert_eq!(r4.compat_state(), InstanceState::Completed);
    }

    #[test]
    fn error_boundary_catches_task_failure() {
        let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="risky_task"/>
    <serviceTask id="risky_task"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="risky_task" targetRef="happy_end"/>
    <boundaryEvent id="boundary1" attachedToRef="risky_task">
      <outgoing>f3</outgoing>
      <errorEventDefinition/>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="boundary1" targetRef="error_task"/>
    <serviceTask id="error_task"><outgoing>f4</outgoing></serviceTask>
    <sequenceFlow id="f4" sourceRef="error_task" targetRef="error_end"/>
    <endEvent id="happy_end"/>
    <endEvent id="error_end"/>
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let mut engine = Engine::new(def);
        let r = engine.start(Default::default()).unwrap();
        assert_eq!(r.compat_state(), InstanceState::WaitingForTask);
        assert!(r.active_element_ids().contains(&"risky_task".to_string()));

        // Fail the task — boundary should catch it
        let r2 = engine.fail_task("risky_task", None).unwrap();
        assert_eq!(r2.compat_state(), InstanceState::WaitingForTask);
        assert!(r2.active_element_ids().contains(&"error_task".to_string()));
    }

    #[test]
    fn task_without_boundary_fails_instance() {
        let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="t1"/>
    <serviceTask id="t1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="t1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let mut engine = Engine::new(def);
        engine.start(Default::default()).unwrap();
        let r = engine.fail_task("t1", None).unwrap();
        assert_eq!(r.compat_state(), InstanceState::Failed);
    }

    #[test]
    fn resume_places_token_for_second_task_in_sequential_process() {
        let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="t1"/>
    <serviceTask id="t1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="t1" targetRef="t2"/>
    <serviceTask id="t2"><outgoing>f3</outgoing></serviceTask>
    <sequenceFlow id="f3" sourceRef="t2" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();

        // Session 1: start and complete t1
        let mut engine1 = Engine::new(def.clone());
        let r1 = engine1.start(Default::default()).unwrap();
        assert_eq!(r1.active_element_ids(), vec!["t1".to_string()]);
        let r2 = engine1.complete_task("t1", Default::default()).unwrap();
        assert_eq!(r2.compat_state(), InstanceState::WaitingForTask);
        assert_eq!(r2.active_element_ids(), vec!["t2".to_string()]);

        // Session 2: resume with saved active_element_ids (simulates server rebuild)
        let mut engine2 = Engine::new(def);
        engine2
            .resume(r2.variables.clone(), r2.active_element_ids())
            .unwrap();

        let r3 = engine2.complete_task("t2", Default::default()).unwrap();
        assert_eq!(r3.compat_state(), InstanceState::Completed);
    }

    #[test]
    fn subprocess_runs_inner_tasks_before_continuing() {
        let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="sub1"/>
    <subProcess id="sub1">
      <outgoing>f2</outgoing>
      <startEvent id="sub_s"><outgoing>sf1</outgoing></startEvent>
      <sequenceFlow id="sf1" sourceRef="sub_s" targetRef="inner_task"/>
      <serviceTask id="inner_task"><outgoing>sf2</outgoing></serviceTask>
      <sequenceFlow id="sf2" sourceRef="inner_task" targetRef="sub_e"/>
      <endEvent id="sub_e"/>
    </subProcess>
    <sequenceFlow id="f2" sourceRef="sub1" targetRef="outer_task"/>
    <serviceTask id="outer_task"><outgoing>f3</outgoing></serviceTask>
    <sequenceFlow id="f3" sourceRef="outer_task" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let mut engine = Engine::new(def);
        let r = engine.start(Default::default()).unwrap();
        assert_eq!(r.compat_state(), InstanceState::WaitingForTask);
        assert!(r.active_element_ids().contains(&"inner_task".to_string()));

        let r2 = engine
            .complete_task("inner_task", Default::default())
            .unwrap();
        assert_eq!(r2.compat_state(), InstanceState::WaitingForTask);
        assert!(r2.active_element_ids().contains(&"outer_task".to_string()));

        let r3 = engine
            .complete_task("outer_task", Default::default())
            .unwrap();
        assert_eq!(r3.compat_state(), InstanceState::Completed);
    }

    #[test]
    fn test_active_element_has_wait_state() {
        let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="t1"/>
    <serviceTask id="t1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="t1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
        let def = parse_bpmn(xml).unwrap();
        let mut engine = Engine::new(def);
        let result = engine.start(HashMap::new()).unwrap();
        assert_eq!(result.active_elements.len(), 1);
        assert_eq!(result.active_elements[0].element_id, "t1");
        assert!(matches!(
            result.active_elements[0].wait_state,
            WaitState::Task { .. }
        ));
        assert!(!result.is_completed);
        assert!(!result.is_failed);
    }

    #[test]
    fn test_parallel_fork_creates_two_active_elements() {
        let def = parse_bpmn(FORK_JOIN_BPMN).unwrap();
        let mut engine = Engine::new(def);
        let result = engine.start(HashMap::new()).unwrap();
        assert_eq!(result.active_elements.len(), 2);
        let ids: Vec<&str> = result
            .active_elements
            .iter()
            .map(|e| e.element_id.as_str())
            .collect();
        assert!(ids.contains(&"ta"));
        assert!(ids.contains(&"tb"));
    }

    mod receive_task_tests {
        use super::*;
        use crate::parser::parse_bpmn;
        use std::collections::HashMap;

        const RECEIVE_TASK_BPMN: &str = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:zeebe="http://camunda.org/schema/zeebe/1.0">
  <message id="Msg_Order" name="order-received">
    <extensionElements>
      <zeebe:subscription correlationKey="= orderId"/>
    </extensionElements>
  </message>
  <process id="proc1">
    <startEvent id="start1"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start1" targetRef="rt1"/>
    <receiveTask id="rt1" messageRef="Msg_Order"><outgoing>f2</outgoing></receiveTask>
    <sequenceFlow id="f2" sourceRef="rt1" targetRef="end1"/>
    <endEvent id="end1"/>
  </process>
</definitions>"#;

        #[test]
        fn receive_task_sets_waiting_for_message() {
            let def = parse_bpmn(RECEIVE_TASK_BPMN).unwrap();
            let mut engine = Engine::new(def);
            let vars = HashMap::from([("orderId".to_string(), serde_json::json!("order-42"))]);
            let result = engine.start(vars).unwrap();
            match &result.compat_state() {
                InstanceState::WaitingForMessage {
                    element_id,
                    message_name,
                    correlation_key_expr,
                } => {
                    assert_eq!(element_id, "rt1");
                    assert_eq!(message_name, "order-received");
                    assert_eq!(correlation_key_expr.as_deref(), Some("= orderId"));
                }
                other => panic!("Expected WaitingForMessage, got {:?}", other),
            }
            assert_eq!(result.active_element_ids(), vec!["rt1"]);
        }

        #[test]
        fn receive_task_completes_on_message() {
            let def = parse_bpmn(RECEIVE_TASK_BPMN).unwrap();
            let mut engine = Engine::new(def);
            let vars = HashMap::from([("orderId".to_string(), serde_json::json!("order-42"))]);
            engine.start(vars).unwrap();
            let result = engine
                .receive_message(
                    "rt1",
                    HashMap::from([("confirmed".to_string(), serde_json::json!(true))]),
                )
                .unwrap();
            assert_eq!(result.compat_state(), InstanceState::Completed);
        }
    }

    // --- Inclusive Gateway tests ---

    // Inclusive gateway: f2 has condition (amount > 100), f3 is unconditional (always taken).
    // Both branches → task_a and task_b → join back at inclusive join.
    const INCLUSIVE_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="ig">
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="fork"/>
    <inclusiveGateway id="fork">
      <incoming>f1</incoming>
      <outgoing>f2</outgoing>
      <outgoing>f3</outgoing>
    </inclusiveGateway>
    <sequenceFlow id="f2" sourceRef="fork" targetRef="task_a">
      <conditionExpression>amount &gt; 100</conditionExpression>
    </sequenceFlow>
    <sequenceFlow id="f3" sourceRef="fork" targetRef="task_b"/>
    <serviceTask id="task_a" name="Task A">
      <incoming>f2</incoming>
      <outgoing>f4</outgoing>
    </serviceTask>
    <serviceTask id="task_b" name="Task B">
      <incoming>f3</incoming>
      <outgoing>f5</outgoing>
    </serviceTask>
    <sequenceFlow id="f4" sourceRef="task_a" targetRef="join"/>
    <sequenceFlow id="f5" sourceRef="task_b" targetRef="join"/>
    <inclusiveGateway id="join">
      <incoming>f4</incoming>
      <incoming>f5</incoming>
      <outgoing>f6</outgoing>
    </inclusiveGateway>
    <sequenceFlow id="f6" sourceRef="join" targetRef="end"/>
    <endEvent id="end"><incoming>f6</incoming></endEvent>
  </process>
</definitions>"#;

    // Inclusive gateway with default flow: f2 has condition, f3 is default (fallback).
    // When f2's condition is false, only f3 is taken.
    const INCLUSIVE_DEFAULT_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="ig2">
  <process id="p2" isExecutable="true">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="fork"/>
    <inclusiveGateway id="fork" default="f3">
      <incoming>f1</incoming>
      <outgoing>f2</outgoing>
      <outgoing>f3</outgoing>
    </inclusiveGateway>
    <sequenceFlow id="f2" sourceRef="fork" targetRef="task_a">
      <conditionExpression>amount &gt; 100</conditionExpression>
    </sequenceFlow>
    <sequenceFlow id="f3" sourceRef="fork" targetRef="task_b"/>
    <serviceTask id="task_a" name="Task A">
      <incoming>f2</incoming>
      <outgoing>f4</outgoing>
    </serviceTask>
    <serviceTask id="task_b" name="Task B">
      <incoming>f3</incoming>
      <outgoing>f5</outgoing>
    </serviceTask>
    <sequenceFlow id="f4" sourceRef="task_a" targetRef="join"/>
    <sequenceFlow id="f5" sourceRef="task_b" targetRef="join"/>
    <inclusiveGateway id="join">
      <incoming>f4</incoming>
      <incoming>f5</incoming>
      <outgoing>f6</outgoing>
    </inclusiveGateway>
    <sequenceFlow id="f6" sourceRef="join" targetRef="end"/>
    <endEvent id="end"><incoming>f6</incoming></endEvent>
  </process>
</definitions>"#;

    #[test]
    fn test_inclusive_both_branches_activate() {
        let mut engine = Engine::new(parse_bpmn(INCLUSIVE_BPMN).unwrap());
        let mut vars = HashMap::new();
        vars.insert("amount".to_string(), serde_json::json!(200));
        let result = engine.start(vars).unwrap();
        // amount > 100 is true → f2 (task_a) taken; f3 unconditional → task_b also taken
        assert_eq!(result.active_elements.len(), 2);
        let ids: Vec<&str> = result
            .active_elements
            .iter()
            .map(|e| e.element_id.as_str())
            .collect();
        assert!(ids.contains(&"task_a"));
        assert!(ids.contains(&"task_b"));
    }

    #[test]
    fn test_inclusive_condition_false_only_unconditional() {
        let mut engine = Engine::new(parse_bpmn(INCLUSIVE_BPMN).unwrap());
        let mut vars = HashMap::new();
        vars.insert("amount".to_string(), serde_json::json!(50));
        let result = engine.start(vars).unwrap();
        // amount > 100 is false → only f3 (unconditional, task_b) taken
        assert_eq!(result.active_elements.len(), 1);
        assert_eq!(result.active_elements[0].element_id, "task_b");
    }

    #[test]
    fn test_inclusive_default_flow_fallback() {
        let mut engine = Engine::new(parse_bpmn(INCLUSIVE_DEFAULT_BPMN).unwrap());
        let mut vars = HashMap::new();
        vars.insert("amount".to_string(), serde_json::json!(50));
        let result = engine.start(vars).unwrap();
        // With default flow: f2 condition false, no other unconditional → default f3 taken
        assert_eq!(result.active_elements.len(), 1);
        assert_eq!(result.active_elements[0].element_id, "task_b");
    }

    #[test]
    fn test_inclusive_default_not_taken_when_condition_matches() {
        let mut engine = Engine::new(parse_bpmn(INCLUSIVE_DEFAULT_BPMN).unwrap());
        let mut vars = HashMap::new();
        vars.insert("amount".to_string(), serde_json::json!(200));
        let result = engine.start(vars).unwrap();
        // f2 condition true → only f2 taken; default f3 is NOT taken
        assert_eq!(result.active_elements.len(), 1);
        assert_eq!(result.active_elements[0].element_id, "task_a");
    }

    #[test]
    fn test_inclusive_join_waits_for_activated_count() {
        let mut engine = Engine::new(parse_bpmn(INCLUSIVE_BPMN).unwrap());
        let mut vars = HashMap::new();
        vars.insert("amount".to_string(), serde_json::json!(200));
        engine.start(vars).unwrap();

        // Complete task_a — join should wait for task_b
        let r1 = engine.complete_task("task_a", HashMap::new()).unwrap();
        assert!(!r1.is_completed, "Should still wait for task_b");
        assert_eq!(r1.active_elements.len(), 1);
        assert_eq!(r1.active_elements[0].element_id, "task_b");

        // Complete task_b — join fires, instance completes
        let r2 = engine.complete_task("task_b", HashMap::new()).unwrap();
        assert!(r2.is_completed);
    }

    #[test]
    fn test_inclusive_single_branch_completes_at_join() {
        let mut engine = Engine::new(parse_bpmn(INCLUSIVE_BPMN).unwrap());
        let mut vars = HashMap::new();
        vars.insert("amount".to_string(), serde_json::json!(50));
        engine.start(vars).unwrap();

        // Only task_b active; join expects 1 → completes immediately
        let r = engine.complete_task("task_b", HashMap::new()).unwrap();
        assert!(r.is_completed);
    }

    const EBG_BPMN: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL"
                  xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
                  targetNamespace="http://test">
  <bpmn:message id="msg1" name="OrderReceived"/>
  <bpmn:process id="p1" isExecutable="true">
    <bpmn:startEvent id="start"><bpmn:outgoing>f1</bpmn:outgoing></bpmn:startEvent>
    <bpmn:eventBasedGateway id="ebg">
      <bpmn:incoming>f1</bpmn:incoming>
      <bpmn:outgoing>f2</bpmn:outgoing>
      <bpmn:outgoing>f3</bpmn:outgoing>
    </bpmn:eventBasedGateway>
    <bpmn:intermediateCatchEvent id="msg_catch">
      <bpmn:incoming>f2</bpmn:incoming><bpmn:outgoing>f4</bpmn:outgoing>
      <bpmn:messageEventDefinition messageRef="msg1"/>
    </bpmn:intermediateCatchEvent>
    <bpmn:intermediateCatchEvent id="timer_catch">
      <bpmn:incoming>f3</bpmn:incoming><bpmn:outgoing>f5</bpmn:outgoing>
      <bpmn:timerEventDefinition><bpmn:timeDuration>PT1H</bpmn:timeDuration></bpmn:timerEventDefinition>
    </bpmn:intermediateCatchEvent>
    <bpmn:serviceTask id="task_msg"><bpmn:incoming>f4</bpmn:incoming><bpmn:outgoing>f6</bpmn:outgoing></bpmn:serviceTask>
    <bpmn:serviceTask id="task_timer"><bpmn:incoming>f5</bpmn:incoming><bpmn:outgoing>f7</bpmn:outgoing></bpmn:serviceTask>
    <bpmn:endEvent id="end_msg"><bpmn:incoming>f6</bpmn:incoming></bpmn:endEvent>
    <bpmn:endEvent id="end_timer"><bpmn:incoming>f7</bpmn:incoming></bpmn:endEvent>
    <bpmn:sequenceFlow id="f1" sourceRef="start" targetRef="ebg"/>
    <bpmn:sequenceFlow id="f2" sourceRef="ebg" targetRef="msg_catch"/>
    <bpmn:sequenceFlow id="f3" sourceRef="ebg" targetRef="timer_catch"/>
    <bpmn:sequenceFlow id="f4" sourceRef="msg_catch" targetRef="task_msg"/>
    <bpmn:sequenceFlow id="f5" sourceRef="timer_catch" targetRef="task_timer"/>
    <bpmn:sequenceFlow id="f6" sourceRef="task_msg" targetRef="end_msg"/>
    <bpmn:sequenceFlow id="f7" sourceRef="task_timer" targetRef="end_timer"/>
  </bpmn:process>
</bpmn:definitions>"#;

    #[test]
    fn test_event_based_gateway_creates_two_waiting_elements() {
        let mut engine = Engine::new(parse_bpmn(EBG_BPMN).unwrap());
        let result = engine.start(HashMap::new()).unwrap();
        assert_eq!(result.active_elements.len(), 2);
        let ids: Vec<&str> = result
            .active_elements
            .iter()
            .map(|e| e.element_id.as_str())
            .collect();
        assert!(ids.contains(&"msg_catch"));
        assert!(ids.contains(&"timer_catch"));
        let has_message = result
            .active_elements
            .iter()
            .any(|e| matches!(e.wait_state, WaitState::Message { .. }));
        let has_timer = result
            .active_elements
            .iter()
            .any(|e| matches!(e.wait_state, WaitState::Timer { .. }));
        assert!(has_message);
        assert!(has_timer);
    }

    // ---- Task 4 tests ----

    #[test]
    fn interrupting_error_esp_catches_error_from_task_failure() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <error id="Err1" errorCode="TASK_ERROR"/>
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="esp1" triggeredByEvent="true">
      <startEvent id="esp_start" isInterrupting="true">
        <errorEventDefinition errorRef="Err1"/>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="esp_start" targetRef="esp_task"/>
      <serviceTask id="esp_task"><outgoing>sf_e2</outgoing></serviceTask>
      <sequenceFlow id="sf_e2" sourceRef="esp_task" targetRef="esp_end"/>
      <endEvent id="esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

        let def = crate::parser::parse_bpmn(xml).unwrap();
        let mut engine = Engine::new(def);
        let start_result = engine.start(std::collections::HashMap::new()).unwrap();
        assert_eq!(start_result.active_elements.len(), 1);
        assert_eq!(start_result.active_elements[0].element_id, "task1");

        let fail_result = engine
            .fail_task("task1", Some("TASK_ERROR".to_string()))
            .unwrap();
        assert!(!fail_result.is_failed);
        assert!(
            fail_result
                .active_elements
                .iter()
                .any(|e| e.element_id == "esp_task"),
            "esp_task should be active after error ESP triggered, got: {:?}",
            fail_result.active_elements
        );
    }

    #[test]
    fn error_esp_catch_all_catches_any_error_code() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="esp1" triggeredByEvent="true">
      <startEvent id="esp_start" isInterrupting="true">
        <errorEventDefinition/>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="esp_start" targetRef="esp_end"/>
      <endEvent id="esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

        let def = crate::parser::parse_bpmn(xml).unwrap();
        let mut engine = Engine::new(def);
        engine.start(std::collections::HashMap::new()).unwrap();
        let result = engine
            .fail_task("task1", Some("ANY_ERROR".to_string()))
            .unwrap();
        assert!(
            !result.is_failed,
            "catch-all error ESP should handle any error code"
        );
        assert!(result.is_completed);
    }

    // ---- Task 5 tests ----

    #[test]
    fn interrupting_escalation_esp_catches_escalation_from_subprocess() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <escalation id="Esc1" escalationCode="NEED_APPROVAL"/>
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="sub1"/>
    <subProcess id="sub1"><outgoing>sf2</outgoing><incoming>sf1</incoming>
      <startEvent id="sub_start"><outgoing>sf3</outgoing></startEvent>
      <sequenceFlow id="sf3" sourceRef="sub_start" targetRef="esc_throw"/>
      <intermediateThrowEvent id="esc_throw"><outgoing>sf4</outgoing>
        <escalationEventDefinition escalationRef="Esc1"/>
      </intermediateThrowEvent>
      <sequenceFlow id="sf4" sourceRef="esc_throw" targetRef="sub_end"/>
      <endEvent id="sub_end"/>
    </subProcess>
    <sequenceFlow id="sf2" sourceRef="sub1" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="esp1" triggeredByEvent="true">
      <startEvent id="esp_start" isInterrupting="true">
        <escalationEventDefinition escalationRef="Esc1"/>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="esp_start" targetRef="esp_task"/>
      <serviceTask id="esp_task"><outgoing>sf_e2</outgoing></serviceTask>
      <sequenceFlow id="sf_e2" sourceRef="esp_task" targetRef="esp_end"/>
      <endEvent id="esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

        let def = crate::parser::parse_bpmn(xml).unwrap();
        let mut engine = Engine::new(def);
        let result = engine.start(std::collections::HashMap::new()).unwrap();
        assert!(
            result
                .active_elements
                .iter()
                .any(|e| e.element_id == "esp_task"),
            "esp_task should be active, got: {:?}",
            result.active_elements
        );
    }

    #[test]
    fn non_interrupting_escalation_esp_runs_in_parallel() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <escalation id="Esc1" escalationCode="NOTIFY"/>
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="esc_throw"/>
    <intermediateThrowEvent id="esc_throw"><outgoing>sf3</outgoing>
      <escalationEventDefinition escalationRef="Esc1"/>
    </intermediateThrowEvent>
    <sequenceFlow id="sf3" sourceRef="esc_throw" targetRef="task2"/>
    <serviceTask id="task2"><outgoing>sf4</outgoing></serviceTask>
    <sequenceFlow id="sf4" sourceRef="task2" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="esp1" triggeredByEvent="true">
      <startEvent id="esp_start" isInterrupting="false">
        <escalationEventDefinition escalationRef="Esc1"/>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="esp_start" targetRef="esp_task"/>
      <serviceTask id="esp_task"><outgoing>sf_e2</outgoing></serviceTask>
      <sequenceFlow id="sf_e2" sourceRef="esp_task" targetRef="esp_end"/>
      <endEvent id="esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

        let def = crate::parser::parse_bpmn(xml).unwrap();
        let mut engine = Engine::new(def);
        let start = engine.start(std::collections::HashMap::new()).unwrap();
        assert_eq!(start.active_elements[0].element_id, "task1");
        // Complete task1 so execution reaches esc_throw then task2
        let result = engine
            .complete_task("task1", std::collections::HashMap::new())
            .unwrap();
        let ids: Vec<_> = result
            .active_elements
            .iter()
            .map(|e| e.element_id.as_str())
            .collect();
        assert!(
            ids.contains(&"task2"),
            "task2 should still be active: {:?}",
            ids
        );
        assert!(
            ids.contains(&"esp_task"),
            "esp_task should be active in parallel: {:?}",
            ids
        );
    }

    // ---- Task 6 tests ----

    #[test]
    fn trigger_event_subprocess_by_id_interrupting() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <message id="Msg1" name="cancel"/>
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="esp1" triggeredByEvent="true">
      <startEvent id="esp_start" isInterrupting="true">
        <messageEventDefinition messageRef="Msg1"/>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="esp_start" targetRef="esp_task"/>
      <serviceTask id="esp_task"><outgoing>sf_e2</outgoing></serviceTask>
      <sequenceFlow id="sf_e2" sourceRef="esp_task" targetRef="esp_end"/>
      <endEvent id="esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

        let def = crate::parser::parse_bpmn(xml).unwrap();
        let mut engine = Engine::new(def);
        let start = engine.start(std::collections::HashMap::new()).unwrap();
        assert_eq!(start.active_elements[0].element_id, "task1");

        let result = engine
            .trigger_event_subprocess("esp1", std::collections::HashMap::new())
            .unwrap();
        assert!(
            !result
                .active_elements
                .iter()
                .any(|e| e.element_id == "task1"),
            "task1 should be cancelled"
        );
        assert!(
            result
                .active_elements
                .iter()
                .any(|e| e.element_id == "esp_task"),
            "esp_task should be active"
        );
    }

    #[test]
    fn trigger_event_subprocess_by_id_non_interrupting() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <signal id="Sig1" name="alert"/>
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="esp1" triggeredByEvent="true">
      <startEvent id="esp_start" isInterrupting="false">
        <signalEventDefinition signalRef="Sig1"/>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="esp_start" targetRef="esp_task"/>
      <serviceTask id="esp_task"><outgoing>sf_e2</outgoing></serviceTask>
      <sequenceFlow id="sf_e2" sourceRef="esp_task" targetRef="esp_end"/>
      <endEvent id="esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

        let def = crate::parser::parse_bpmn(xml).unwrap();
        let mut engine = Engine::new(def);
        engine.start(std::collections::HashMap::new()).unwrap();
        let result = engine
            .trigger_event_subprocess("esp1", std::collections::HashMap::new())
            .unwrap();
        let ids: Vec<_> = result
            .active_elements
            .iter()
            .map(|e| e.element_id.as_str())
            .collect();
        assert!(
            ids.contains(&"task1"),
            "task1 should still be active: {:?}",
            ids
        );
        assert!(
            ids.contains(&"esp_task"),
            "esp_task should run in parallel: {:?}",
            ids
        );
    }

    #[test]
    fn event_subprocess_message_trigger_collected_in_subscriptions() {
        use crate::model::*;
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <message id="Msg1" name="cancel-order"/>
  <process id="p1" isExecutable="true">
    <startEvent id="start"><outgoing>sf1</outgoing></startEvent>
    <sequenceFlow id="sf1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
    <subProcess id="esp1" triggeredByEvent="true">
      <startEvent id="esp_start" isInterrupting="true">
        <messageEventDefinition messageRef="Msg1"/>
        <outgoing>sf_e1</outgoing>
      </startEvent>
      <sequenceFlow id="sf_e1" sourceRef="esp_start" targetRef="esp_end"/>
      <endEvent id="esp_end"/>
    </subProcess>
  </process>
</definitions>"#;

        let def = crate::parser::parse_bpmn(xml).unwrap();
        let mut engine = Engine::new(def);
        let result = engine.start(std::collections::HashMap::new()).unwrap();

        assert_eq!(result.event_subprocess_subscriptions.len(), 1);
        let sub = &result.event_subprocess_subscriptions[0];
        assert_eq!(sub.esp_id, "esp1");
        assert!(sub.is_interrupting);
        assert!(sub.scope_id.is_none());
        assert!(
            matches!(sub.trigger, EventSubProcessTrigger::Message { ref message_name, .. } if message_name == "cancel-order")
        );
    }
}
