use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextAnnotation {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Association {
    pub id: String,
    pub source_ref: String,
    pub target_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessDefinition {
    pub id: String,
    pub name: Option<String>,
    pub elements: Vec<FlowElement>,
    pub sequence_flows: Vec<SequenceFlow>,
    #[serde(default)]
    pub annotations: Vec<TextAnnotation>,
    #[serde(default)]
    pub associations: Vec<Association>,
}

impl ProcessDefinition {
    /// Return the external task topic for a service task element, if any.
    pub fn element_topic(&self, element_id: &str) -> Option<&str> {
        self.elements
            .iter()
            .find(|e| e.id() == element_id)
            .and_then(|e| match e {
                FlowElement::ServiceTask(t) => t.topic.as_deref(),
                _ => None,
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FlowElement {
    StartEvent(StartEvent),
    TimerStartEvent(TimerStartEvent),
    EndEvent(EndEvent),
    ServiceTask(ServiceTask),
    ExclusiveGateway(ExclusiveGateway),
    TimerIntermediateEvent(TimerIntermediateEvent),
    ParallelGateway(ParallelGateway),
    InclusiveGateway(InclusiveGateway),
    MultiInstanceTask(MultiInstanceTask),
    SubProcess(SubProcess),
    BoundaryEvent(BoundaryEvent),
    MessageIntermediateCatchEvent(MessageIntermediateCatchEvent),
    SignalIntermediateCatchEvent(SignalIntermediateCatchEvent),
    /// BPMN intermediateThrowEvent — treated as a pass-through milestone (immediately advances).
    IntermediateThrowEvent(IntermediateThrowEvent),
    MessageStartEvent(MessageStartEvent),
    MessageBoundaryEvent(MessageBoundaryEvent),
    TimerBoundaryEvent(TimerBoundaryEvent),
    SignalStartEvent(SignalStartEvent),
    SignalIntermediateThrowEvent(SignalIntermediateThrowEvent),
    SignalEndEvent(SignalEndEvent),
    SignalBoundaryEvent(SignalBoundaryEvent),
    MessageEndEvent(MessageEndEvent),
    MessageIntermediateThrowEvent(MessageIntermediateThrowEvent),
    ReceiveTask(ReceiveTask),
    EventBasedGateway(EventBasedGateway),
    ScriptTask(ScriptTask),
    ErrorEndEvent(ErrorEndEvent),
    TerminateEndEvent(TerminateEndEvent),
    EscalationIntermediateThrowEvent(EscalationIntermediateThrowEvent),
    EscalationEndEvent(EscalationEndEvent),
    EscalationBoundaryEvent(EscalationBoundaryEvent),
    LinkIntermediateThrowEvent(LinkIntermediateThrowEvent),
    LinkIntermediateCatchEvent(LinkIntermediateCatchEvent),
    EventSubProcess(EventSubProcess),
    EventSubProcessStartEvent(EventSubProcessStartEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VisitEvent {
    Activated,
    Completed,
    ErrorThrown,
    EscalationThrown,
    MessageThrown,
    LinkJumped,
    Terminated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisitedElement {
    pub element_id: String,
    pub element_name: Option<String>,
    pub element_type: String,
    pub event: VisitEvent,
}

impl FlowElement {
    pub fn id(&self) -> &str {
        match self {
            FlowElement::StartEvent(e) => &e.id,
            FlowElement::TimerStartEvent(e) => &e.id,
            FlowElement::EndEvent(e) => &e.id,
            FlowElement::ServiceTask(e) => &e.id,
            FlowElement::ExclusiveGateway(e) => &e.id,
            FlowElement::TimerIntermediateEvent(e) => &e.id,
            FlowElement::ParallelGateway(e) => &e.id,
            FlowElement::InclusiveGateway(e) => &e.id,
            FlowElement::MultiInstanceTask(e) => &e.id,
            FlowElement::SubProcess(e) => &e.id,
            FlowElement::BoundaryEvent(e) => &e.id,
            FlowElement::MessageIntermediateCatchEvent(e) => &e.id,
            FlowElement::SignalIntermediateCatchEvent(e) => &e.id,
            FlowElement::IntermediateThrowEvent(e) => &e.id,
            FlowElement::MessageStartEvent(e) => &e.id,
            FlowElement::MessageBoundaryEvent(e) => &e.id,
            FlowElement::TimerBoundaryEvent(e) => &e.id,
            FlowElement::SignalStartEvent(e) => &e.id,
            FlowElement::SignalIntermediateThrowEvent(e) => &e.id,
            FlowElement::SignalEndEvent(e) => &e.id,
            FlowElement::SignalBoundaryEvent(e) => &e.id,
            FlowElement::MessageEndEvent(e) => &e.id,
            FlowElement::MessageIntermediateThrowEvent(e) => &e.id,
            FlowElement::ReceiveTask(e) => &e.id,
            FlowElement::EventBasedGateway(e) => &e.id,
            FlowElement::ScriptTask(e) => &e.id,
            FlowElement::ErrorEndEvent(e) => &e.id,
            FlowElement::TerminateEndEvent(e) => &e.id,
            FlowElement::EscalationIntermediateThrowEvent(e) => &e.id,
            FlowElement::EscalationEndEvent(e) => &e.id,
            FlowElement::EscalationBoundaryEvent(e) => &e.id,
            FlowElement::LinkIntermediateThrowEvent(e) => &e.id,
            FlowElement::LinkIntermediateCatchEvent(e) => &e.id,
            FlowElement::EventSubProcess(e) => &e.id,
            FlowElement::EventSubProcessStartEvent(e) => &e.id,
        }
    }

    pub fn outgoing(&self) -> &[String] {
        match self {
            FlowElement::StartEvent(e) => &e.outgoing,
            FlowElement::TimerStartEvent(e) => &e.outgoing,
            FlowElement::EndEvent(e) => &e.outgoing,
            FlowElement::ServiceTask(e) => &e.outgoing,
            FlowElement::ExclusiveGateway(e) => &e.outgoing,
            FlowElement::TimerIntermediateEvent(e) => &e.outgoing,
            FlowElement::ParallelGateway(e) => &e.outgoing,
            FlowElement::InclusiveGateway(e) => &e.outgoing,
            FlowElement::MultiInstanceTask(e) => &e.outgoing,
            FlowElement::SubProcess(e) => &e.outgoing,
            FlowElement::BoundaryEvent(e) => &e.outgoing,
            FlowElement::MessageIntermediateCatchEvent(e) => &e.outgoing,
            FlowElement::SignalIntermediateCatchEvent(e) => &e.outgoing,
            FlowElement::IntermediateThrowEvent(e) => &e.outgoing,
            FlowElement::MessageStartEvent(e) => &e.outgoing,
            FlowElement::MessageBoundaryEvent(e) => &e.outgoing,
            FlowElement::TimerBoundaryEvent(e) => &e.outgoing,
            FlowElement::SignalStartEvent(e) => &e.outgoing,
            FlowElement::SignalIntermediateThrowEvent(e) => &e.outgoing,
            FlowElement::SignalEndEvent(e) => &e.outgoing,
            FlowElement::SignalBoundaryEvent(e) => &e.outgoing,
            FlowElement::MessageEndEvent(e) => &e.outgoing,
            FlowElement::MessageIntermediateThrowEvent(e) => &e.outgoing,
            FlowElement::ReceiveTask(e) => &e.outgoing,
            FlowElement::EventBasedGateway(e) => &e.outgoing,
            FlowElement::ScriptTask(e) => &e.outgoing,
            FlowElement::ErrorEndEvent(e) => &e.outgoing,
            FlowElement::TerminateEndEvent(e) => &e.outgoing,
            FlowElement::EscalationIntermediateThrowEvent(e) => &e.outgoing,
            FlowElement::EscalationEndEvent(e) => &e.outgoing,
            FlowElement::EscalationBoundaryEvent(e) => &e.outgoing,
            FlowElement::LinkIntermediateThrowEvent(e) => &e.outgoing,
            FlowElement::LinkIntermediateCatchEvent(e) => &e.outgoing,
            FlowElement::EventSubProcess(_) => &[],
            FlowElement::EventSubProcessStartEvent(e) => &e.outgoing,
        }
    }

    pub fn incoming(&self) -> &[String] {
        match self {
            FlowElement::StartEvent(_) => &[],
            FlowElement::TimerStartEvent(_) => &[],
            FlowElement::EndEvent(_) => &[],
            FlowElement::ServiceTask(_) => &[],
            FlowElement::ExclusiveGateway(_) => &[],
            FlowElement::TimerIntermediateEvent(_) => &[],
            FlowElement::ParallelGateway(e) => &e.incoming,
            FlowElement::InclusiveGateway(e) => &e.incoming,
            FlowElement::MultiInstanceTask(e) => &e.incoming,
            FlowElement::SubProcess(e) => &e.incoming,
            FlowElement::BoundaryEvent(_) => &[],
            FlowElement::MessageIntermediateCatchEvent(_) => &[],
            FlowElement::SignalIntermediateCatchEvent(_) => &[],
            FlowElement::IntermediateThrowEvent(_) => &[],
            FlowElement::MessageStartEvent(_) => &[],
            FlowElement::MessageBoundaryEvent(_) => &[],
            FlowElement::TimerBoundaryEvent(_) => &[],
            FlowElement::SignalStartEvent(_) => &[],
            FlowElement::SignalIntermediateThrowEvent(_) => &[],
            FlowElement::SignalEndEvent(_) => &[],
            FlowElement::SignalBoundaryEvent(_) => &[],
            FlowElement::MessageEndEvent(_) => &[],
            FlowElement::MessageIntermediateThrowEvent(_) => &[],
            FlowElement::ReceiveTask(_) => &[],
            FlowElement::EventBasedGateway(e) => &e.incoming,
            FlowElement::ScriptTask(_) => &[],
            FlowElement::ErrorEndEvent(_) => &[],
            FlowElement::TerminateEndEvent(_) => &[],
            FlowElement::EscalationIntermediateThrowEvent(_) => &[],
            FlowElement::EscalationEndEvent(_) => &[],
            FlowElement::EscalationBoundaryEvent(_) => &[],
            FlowElement::LinkIntermediateThrowEvent(_) => &[],
            FlowElement::LinkIntermediateCatchEvent(_) => &[],
            FlowElement::EventSubProcess(_) => &[],
            FlowElement::EventSubProcessStartEvent(_) => &[],
        }
    }

    pub fn name(&self) -> Option<&str> {
        match self {
            FlowElement::StartEvent(e) => e.name.as_deref(),
            FlowElement::TimerStartEvent(e) => e.name.as_deref(),
            FlowElement::EndEvent(e) => e.name.as_deref(),
            FlowElement::ServiceTask(e) => e.name.as_deref(),
            FlowElement::ExclusiveGateway(e) => e.name.as_deref(),
            FlowElement::TimerIntermediateEvent(e) => e.name.as_deref(),
            FlowElement::ParallelGateway(e) => e.name.as_deref(),
            FlowElement::InclusiveGateway(e) => e.name.as_deref(),
            FlowElement::MultiInstanceTask(e) => e.name.as_deref(),
            FlowElement::SubProcess(e) => e.name.as_deref(),
            FlowElement::BoundaryEvent(e) => e.name.as_deref(),
            FlowElement::MessageIntermediateCatchEvent(e) => e.name.as_deref(),
            FlowElement::SignalIntermediateCatchEvent(e) => e.name.as_deref(),
            FlowElement::IntermediateThrowEvent(e) => e.name.as_deref(),
            FlowElement::MessageStartEvent(e) => e.name.as_deref(),
            FlowElement::MessageBoundaryEvent(e) => e.name.as_deref(),
            FlowElement::TimerBoundaryEvent(e) => e.name.as_deref(),
            FlowElement::SignalStartEvent(e) => e.name.as_deref(),
            FlowElement::SignalIntermediateThrowEvent(e) => e.name.as_deref(),
            FlowElement::SignalEndEvent(e) => e.name.as_deref(),
            FlowElement::SignalBoundaryEvent(e) => e.name.as_deref(),
            FlowElement::MessageEndEvent(e) => e.name.as_deref(),
            FlowElement::MessageIntermediateThrowEvent(e) => e.name.as_deref(),
            FlowElement::ReceiveTask(e) => e.name.as_deref(),
            FlowElement::EventBasedGateway(e) => e.name.as_deref(),
            FlowElement::ScriptTask(e) => e.name.as_deref(),
            FlowElement::ErrorEndEvent(e) => e.name.as_deref(),
            FlowElement::TerminateEndEvent(e) => e.name.as_deref(),
            FlowElement::EscalationIntermediateThrowEvent(e) => e.name.as_deref(),
            FlowElement::EscalationEndEvent(e) => e.name.as_deref(),
            FlowElement::EscalationBoundaryEvent(e) => e.name.as_deref(),
            FlowElement::LinkIntermediateThrowEvent(e) => e.name.as_deref(),
            FlowElement::LinkIntermediateCatchEvent(e) => e.name.as_deref(),
            FlowElement::EventSubProcess(e) => e.name.as_deref(),
            FlowElement::EventSubProcessStartEvent(e) => e.name.as_deref(),
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            FlowElement::StartEvent(_) => "StartEvent",
            FlowElement::TimerStartEvent(_) => "TimerStartEvent",
            FlowElement::EndEvent(_) => "EndEvent",
            FlowElement::ServiceTask(_) => "ServiceTask",
            FlowElement::ExclusiveGateway(_) => "ExclusiveGateway",
            FlowElement::TimerIntermediateEvent(_) => "TimerIntermediateEvent",
            FlowElement::ParallelGateway(_) => "ParallelGateway",
            FlowElement::InclusiveGateway(_) => "InclusiveGateway",
            FlowElement::MultiInstanceTask(_) => "MultiInstanceTask",
            FlowElement::SubProcess(_) => "SubProcess",
            FlowElement::BoundaryEvent(_) => "BoundaryEvent",
            FlowElement::MessageIntermediateCatchEvent(_) => "MessageIntermediateCatchEvent",
            FlowElement::SignalIntermediateCatchEvent(_) => "SignalIntermediateCatchEvent",
            FlowElement::IntermediateThrowEvent(_) => "IntermediateThrowEvent",
            FlowElement::MessageStartEvent(_) => "MessageStartEvent",
            FlowElement::MessageBoundaryEvent(_) => "MessageBoundaryEvent",
            FlowElement::TimerBoundaryEvent(_) => "TimerBoundaryEvent",
            FlowElement::SignalStartEvent(_) => "SignalStartEvent",
            FlowElement::SignalIntermediateThrowEvent(_) => "SignalIntermediateThrowEvent",
            FlowElement::SignalEndEvent(_) => "SignalEndEvent",
            FlowElement::SignalBoundaryEvent(_) => "SignalBoundaryEvent",
            FlowElement::MessageEndEvent(_) => "MessageEndEvent",
            FlowElement::MessageIntermediateThrowEvent(_) => "MessageIntermediateThrowEvent",
            FlowElement::ReceiveTask(_) => "ReceiveTask",
            FlowElement::EventBasedGateway(_) => "EventBasedGateway",
            FlowElement::ScriptTask(_) => "ScriptTask",
            FlowElement::ErrorEndEvent(_) => "ErrorEndEvent",
            FlowElement::TerminateEndEvent(_) => "TerminateEndEvent",
            FlowElement::EscalationIntermediateThrowEvent(_) => "EscalationIntermediateThrowEvent",
            FlowElement::EscalationEndEvent(_) => "EscalationEndEvent",
            FlowElement::EscalationBoundaryEvent(_) => "EscalationBoundaryEvent",
            FlowElement::LinkIntermediateThrowEvent(_) => "LinkIntermediateThrowEvent",
            FlowElement::LinkIntermediateCatchEvent(_) => "LinkIntermediateCatchEvent",
            FlowElement::EventSubProcess(_) => "EventSubProcess",
            FlowElement::EventSubProcessStartEvent(_) => "EventSubProcessStartEvent",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExclusiveGateway {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    /// The flow ID to take when no condition matches
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum TimerKind {
    #[default]
    Duration,
    Date,
    Cycle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimerDefinition {
    pub kind: TimerKind,
    pub expression: String,
}

impl TimerDefinition {
    /// Fallback for timer elements with no explicit definition.
    pub fn zero_duration() -> Self {
        TimerDefinition {
            kind: TimerKind::Duration,
            expression: "PT0S".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventSubProcessTrigger {
    Error {
        error_code: Option<String>,
    },
    Escalation {
        escalation_code: Option<String>,
    },
    Message {
        message_name: String,
        correlation_key: Option<String>,
    },
    Signal {
        signal_ref: String,
    },
    Timer {
        timer: TimerDefinition,
    },
}

/// The trigger start event of an EventSubProcess.
/// Lives in EventSubProcess.inner_elements so DI rendering picks it up automatically.
/// Never directly advanced to by the engine — trigger is read at ESP activation time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSubProcessStartEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub trigger: EventSubProcessTrigger,
    pub is_interrupting: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSubProcess {
    pub id: String,
    pub name: Option<String>,
    pub inner_elements: Vec<FlowElement>, // includes EventSubProcessStartEvent
    pub inner_flows: Vec<SequenceFlow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSubProcessSubscription {
    pub esp_id: String,
    pub trigger: EventSubProcessTrigger, // Message | Signal | Timer only
    pub scope_id: Option<String>,        // None = root; Some(id) = inside that subprocess
    pub is_interrupting: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerIntermediateEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    /// Timer definition from BPMN. `None` when no timerEventDefinition child is present.
    pub timer: Option<TimerDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageIntermediateCatchEvent {
    pub id: String,
    pub name: Option<String>,
    /// The message name to wait for
    pub message_name: String,
    /// Optional FEEL/BPMN EL expression to evaluate for correlation key, e.g. "= orderId"
    pub correlation_key: Option<String>,
    pub outgoing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiveTask {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub message_name: String,
    pub correlation_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptTask {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub incoming: Vec<String>,
    pub script_format: String,
    pub script: String,
    pub result_variable: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalIntermediateCatchEvent {
    pub id: String,
    pub name: Option<String>,
    /// The signal name/ref to wait for (matches against POST /v1/signals/{name})
    pub signal_ref: String,
    pub outgoing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerStartEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub timer: Option<TimerDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceTask {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    #[serde(default)]
    pub topic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    /// The element ID this boundary is attached to
    pub attached_to_ref: String,
    /// "error", "timer", "message"
    pub event_type: String,
    /// Resolved BPMN error code for error boundary events (from `<error errorCode="..."/>`)
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubProcess {
    pub id: String,
    pub name: Option<String>,
    pub incoming: Vec<String>,
    pub outgoing: Vec<String>,
    /// Inner process elements (nested BPMN)
    pub inner_elements: Vec<FlowElement>,
    pub inner_flows: Vec<SequenceFlow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiInstanceTask {
    pub id: String,
    pub name: Option<String>,
    pub incoming: Vec<String>,
    pub outgoing: Vec<String>,
    /// Variable name containing the input collection
    pub loop_data_input_ref: String,
    /// true = sequential, false = parallel
    pub is_sequential: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelGateway {
    pub id: String,
    pub name: Option<String>,
    pub incoming: Vec<String>,
    pub outgoing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InclusiveGateway {
    pub id: String,
    pub name: Option<String>,
    pub incoming: Vec<String>,
    pub outgoing: Vec<String>,
    /// The flow ID to take when no condition matches
    pub default: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventBasedGateway {
    pub id: String,
    pub name: Option<String>,
    pub incoming: Vec<String>,
    pub outgoing: Vec<String>,
}

/// A BPMN `intermediateThrowEvent` — immediately passes control to the next element.
/// Semantics: upon activation the engine follows the single outgoing sequence flow without waiting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntermediateThrowEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageStartEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub message_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBoundaryEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub attached_to_ref: String,
    pub message_name: String,
    pub correlation_key: Option<String>,
    pub is_interrupting: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerBoundaryEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub attached_to_ref: String,
    pub timer: TimerDefinition,
    pub is_interrupting: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalStartEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub signal_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalIntermediateThrowEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub signal_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalEndEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub signal_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalBoundaryEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub attached_to_ref: String,
    pub signal_ref: String,
    pub is_interrupting: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEndEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub message_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageIntermediateThrowEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub message_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEndEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    /// Resolved BPMN error code (from `<error errorCode="..."/>`)
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminateEndEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationIntermediateThrowEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    /// Resolved BPMN escalation code (from `<escalation escalationCode="..."/>`)
    pub escalation_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationEndEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub escalation_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationBoundaryEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub attached_to_ref: String,
    pub escalation_code: Option<String>,
    pub is_interrupting: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkIntermediateThrowEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub link_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkIntermediateCatchEvent {
    pub id: String,
    pub name: Option<String>,
    pub outgoing: Vec<String>,
    pub link_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceFlow {
    pub id: String,
    pub source_ref: String,
    pub target_ref: String,
    pub condition_expression: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_event_has_id_and_outgoing() {
        let el = StartEvent {
            id: "start1".to_string(),
            name: None,
            outgoing: vec!["flow1".to_string()],
        };
        assert_eq!(el.id, "start1");
        assert_eq!(el.outgoing, vec!["flow1"]);
    }

    #[test]
    fn sequence_flow_connects_source_to_target() {
        let flow = SequenceFlow {
            id: "flow1".to_string(),
            source_ref: "start1".to_string(),
            target_ref: "task1".to_string(),
            condition_expression: None,
        };
        assert_eq!(flow.source_ref, "start1");
        assert_eq!(flow.target_ref, "task1");
    }

    #[test]
    fn exclusive_gateway_has_id_and_outgoing() {
        let gw = ExclusiveGateway {
            id: "gw1".to_string(),
            name: None,
            outgoing: vec!["flowA".to_string(), "flowB".to_string()],
            default: Some("flowB".to_string()),
        };
        assert_eq!(gw.id, "gw1");
        assert_eq!(gw.outgoing.len(), 2);
        assert_eq!(gw.default, Some("flowB".to_string()));
    }

    #[test]
    fn message_intermediate_catch_event_has_id_and_message_name() {
        let el = MessageIntermediateCatchEvent {
            id: "msg1".to_string(),
            name: None,
            message_name: "OrderApproved".to_string(),
            correlation_key: None,
            outgoing: vec!["f1".to_string()],
        };
        assert_eq!(el.id, "msg1");
        assert_eq!(el.message_name, "OrderApproved");
    }

    #[test]
    fn script_task_model_roundtrip() {
        let st = FlowElement::ScriptTask(ScriptTask {
            id: "Script_1".to_string(),
            name: Some("Calculate".to_string()),
            outgoing: vec!["Flow_1".to_string()],
            incoming: vec!["Flow_0".to_string()],
            script_format: "rhai".to_string(),
            script: "a + b".to_string(),
            result_variable: Some("sum".to_string()),
        });
        assert_eq!(st.id(), "Script_1");
        assert_eq!(st.outgoing(), &["Flow_1".to_string()]);
        assert_eq!(st.incoming(), &[] as &[String]); // incoming returns &[] for ScriptTask
    }

    #[test]
    fn signal_intermediate_catch_event_has_id_and_signal_ref() {
        let el = SignalIntermediateCatchEvent {
            id: "sig1".to_string(),
            name: None,
            signal_ref: "PaymentReceived".to_string(),
            outgoing: vec!["f1".to_string()],
        };
        assert_eq!(el.id, "sig1");
        assert_eq!(el.signal_ref, "PaymentReceived");
    }

    #[test]
    fn service_task_has_optional_topic() {
        let t = ServiceTask {
            id: "t1".to_string(),
            name: None,
            outgoing: vec![],
            topic: Some("payments".to_string()),
        };
        assert_eq!(t.topic.as_deref(), Some("payments"));

        let t2 = ServiceTask {
            id: "t2".to_string(),
            name: None,
            outgoing: vec![],
            topic: None,
        };
        assert!(t2.topic.is_none());
    }

    #[test]
    fn timer_definition_kinds_are_distinct() {
        let d = TimerDefinition {
            kind: TimerKind::Duration,
            expression: "PT5M".to_string(),
        };
        let t = TimerDefinition {
            kind: TimerKind::Date,
            expression: "2026-03-01T12:00:00Z".to_string(),
        };
        let c = TimerDefinition {
            kind: TimerKind::Cycle,
            expression: "R3/PT10H".to_string(),
        };
        assert!(matches!(d.kind, TimerKind::Duration));
        assert!(matches!(t.kind, TimerKind::Date));
        assert!(matches!(c.kind, TimerKind::Cycle));
    }

    #[test]
    fn timer_definition_zero_duration_is_duration_kind() {
        let z = TimerDefinition::zero_duration();
        assert!(matches!(z.kind, TimerKind::Duration));
        assert_eq!(z.expression, "PT0S");
    }

    #[test]
    fn timer_kind_default_is_duration() {
        assert!(matches!(TimerKind::default(), TimerKind::Duration));
    }

    #[test]
    fn message_start_event_has_message_name() {
        let e = MessageStartEvent {
            id: "s1".into(),
            name: None,
            outgoing: vec!["f1".into()],
            message_name: "order-placed".into(),
        };
        assert_eq!(e.message_name, "order-placed");
    }

    #[test]
    fn message_boundary_event_is_interrupting_by_default() {
        let e = MessageBoundaryEvent {
            id: "b1".into(),
            name: None,
            outgoing: vec!["f1".into()],
            attached_to_ref: "task1".into(),
            message_name: "cancel".into(),
            correlation_key: Some("= orderId".into()),
            is_interrupting: true,
        };
        assert!(e.is_interrupting);
        assert_eq!(e.correlation_key.as_deref(), Some("= orderId"));
    }

    #[test]
    fn message_end_event_has_message_name() {
        let e = MessageEndEvent {
            id: "end1".into(),
            name: None,
            outgoing: vec![],
            message_name: "order-completed".into(),
        };
        assert_eq!(e.message_name, "order-completed");
    }

    #[test]
    fn message_intermediate_throw_event_has_message_name() {
        let e = MessageIntermediateThrowEvent {
            id: "throw1".into(),
            name: None,
            outgoing: vec!["f2".into()],
            message_name: "notify".into(),
        };
        assert_eq!(e.message_name, "notify");
    }

    #[test]
    fn receive_task_id_and_outgoing() {
        let task = ReceiveTask {
            id: "rt1".to_string(),
            name: None,
            outgoing: vec!["flow1".to_string()],
            message_name: "OrderMsg".to_string(),
            correlation_key: Some("= orderId".to_string()),
        };
        let elem = FlowElement::ReceiveTask(task);
        assert_eq!(elem.id(), "rt1");
        assert_eq!(elem.outgoing(), &["flow1"]);
        assert_eq!(elem.incoming(), &[] as &[String]);
    }
}
