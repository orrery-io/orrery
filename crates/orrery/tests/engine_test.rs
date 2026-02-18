use orrery::engine::{Engine, EngineError, InstanceState};
use orrery::parser::parse_bpmn;
use serde_json::json;
use std::collections::HashMap;

const MESSAGE_CATCH_BPMN: &str = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="msg1"/>
    <intermediateCatchEvent id="msg1">
      <messageEventDefinition messageRef="OrderApproved"/>
      <outgoing>f2</outgoing>
    </intermediateCatchEvent>
    <sequenceFlow id="f2" sourceRef="msg1" targetRef="t1"/>
    <serviceTask id="t1"><outgoing>f3</outgoing></serviceTask>
    <sequenceFlow id="f3" sourceRef="t1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;

#[test]
fn message_catch_pauses_at_message_event() {
    let def = parse_bpmn(MESSAGE_CATCH_BPMN).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();
    assert!(
        matches!(&result.compat_state(), InstanceState::WaitingForMessage { element_id, message_name, .. }
            if element_id == "msg1" && message_name == "OrderApproved"),
        "expected WaitingForMessage, got {:?}",
        result.compat_state()
    );
    assert_eq!(result.active_element_ids(), vec!["msg1"]);
}

#[test]
fn receive_message_advances_to_next_task() {
    let def = parse_bpmn(MESSAGE_CATCH_BPMN).unwrap();
    let mut engine = Engine::new(def);
    engine.start(Default::default()).unwrap();
    let mut vars = std::collections::HashMap::new();
    vars.insert("order_status".to_string(), serde_json::json!("approved"));
    let result = engine.receive_message("msg1", vars).unwrap();
    assert_eq!(result.compat_state(), InstanceState::WaitingForTask);
    assert_eq!(result.active_element_ids(), vec!["t1"]);
    assert_eq!(
        result.variables.get("order_status"),
        Some(&serde_json::json!("approved"))
    );
}

const SIGNAL_CATCH_BPMN: &str = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="sig1"/>
    <intermediateCatchEvent id="sig1">
      <signalEventDefinition signalRef="PaymentReceived"/>
      <outgoing>f2</outgoing>
    </intermediateCatchEvent>
    <sequenceFlow id="f2" sourceRef="sig1" targetRef="t1"/>
    <serviceTask id="t1"><outgoing>f3</outgoing></serviceTask>
    <sequenceFlow id="f3" sourceRef="t1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;

#[test]
fn signal_catch_pauses_at_signal_event() {
    let def = parse_bpmn(SIGNAL_CATCH_BPMN).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();
    assert!(
        matches!(&result.compat_state(), InstanceState::WaitingForSignal { element_id, signal_ref }
            if element_id == "sig1" && signal_ref == "PaymentReceived"),
        "expected WaitingForSignal, got {:?}",
        result.compat_state()
    );
}

#[test]
fn receive_signal_advances_to_next_task() {
    let def = parse_bpmn(SIGNAL_CATCH_BPMN).unwrap();
    let mut engine = Engine::new(def);
    engine.start(Default::default()).unwrap();
    let result = engine.receive_signal("sig1", Default::default()).unwrap();
    assert_eq!(result.compat_state(), InstanceState::WaitingForTask);
    assert_eq!(result.active_element_ids(), vec!["t1"]);
}

#[test]
fn receive_message_errors_when_not_waiting() {
    let def = parse_bpmn(MESSAGE_CATCH_BPMN).unwrap();
    let mut engine = Engine::new(def);
    // Never started — engine not in WaitingForMessage state
    let err = engine
        .receive_message("msg1", Default::default())
        .unwrap_err();
    assert!(matches!(err, EngineError::NoActiveTask));
}

#[test]
fn receive_message_errors_for_wrong_element_id() {
    let def = parse_bpmn(MESSAGE_CATCH_BPMN).unwrap();
    let mut engine = Engine::new(def);
    engine.start(Default::default()).unwrap();
    // Waiting at "msg1", but we send to a different element id
    let err = engine
        .receive_message("wrong_elem", Default::default())
        .unwrap_err();
    assert!(matches!(err, EngineError::NoActiveTask));
}

#[test]
fn receive_signal_errors_when_not_waiting() {
    let def = parse_bpmn(SIGNAL_CATCH_BPMN).unwrap();
    let mut engine = Engine::new(def);
    // Never started — engine not in WaitingForSignal state
    let err = engine
        .receive_signal("sig1", Default::default())
        .unwrap_err();
    assert!(matches!(err, EngineError::NoActiveTask));
}

#[test]
fn receive_signal_errors_for_wrong_element_id() {
    let def = parse_bpmn(SIGNAL_CATCH_BPMN).unwrap();
    let mut engine = Engine::new(def);
    engine.start(Default::default()).unwrap();
    // Waiting at "sig1", but we send to a different element id
    let err = engine
        .receive_signal("wrong_elem", Default::default())
        .unwrap_err();
    assert!(matches!(err, EngineError::NoActiveTask));
}

fn gateway_bpmn() -> &'static str {
    include_str!("fixtures/exclusive_gateway.bpmn")
}

fn timer_bpmn() -> &'static str {
    include_str!("fixtures/timer_process.bpmn")
}

fn simple_bpmn() -> &'static str {
    include_str!("fixtures/simple.bpmn")
}

#[test]
fn gateway_routes_to_task_a_when_approved() {
    let def = parse_bpmn(gateway_bpmn()).unwrap();
    let mut engine = Engine::new(def);
    let vars = [("approved".to_string(), json!(true))]
        .into_iter()
        .collect();
    let result = engine.start(vars).unwrap();
    assert_eq!(result.compat_state(), InstanceState::WaitingForTask);
    assert_eq!(
        result.active_element_ids().first().map(|s| s.as_str()),
        Some("task_a")
    );
}

#[test]
fn gateway_routes_to_task_b_when_not_approved() {
    let def = parse_bpmn(gateway_bpmn()).unwrap();
    let mut engine = Engine::new(def);
    let vars = [("approved".to_string(), json!(false))]
        .into_iter()
        .collect();
    let result = engine.start(vars).unwrap();
    assert_eq!(result.compat_state(), InstanceState::WaitingForTask);
    assert_eq!(
        result.active_element_ids().first().map(|s| s.as_str()),
        Some("task_b")
    );
}

#[test]
fn gateway_routes_to_first_unconditioned_flow_when_no_match() {
    let def = parse_bpmn(gateway_bpmn()).unwrap();
    let mut engine = Engine::new(def);
    // No variables set — condition "approved == true" won't match; sf3 has no condition → fallback
    let result = engine.start(Default::default()).unwrap();
    assert_eq!(
        result.active_element_ids().first().map(|s| s.as_str()),
        Some("task_b")
    );
}

#[test]
fn timer_process_pauses_at_timer_event() {
    let def = parse_bpmn(timer_bpmn()).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();
    assert!(
        matches!(result.compat_state(), InstanceState::WaitingForTimer { ref element_id, ref definition }
            if element_id == "timer1" && definition.expression == "PT1S"),
        "expected WaitingForTimer at timer1 with PT1S, got {:?}",
        result.compat_state()
    );
}

#[test]
fn fire_timer_advances_to_next_task() {
    let def = parse_bpmn(timer_bpmn()).unwrap();
    let mut engine = Engine::new(def);
    engine.start(Default::default()).unwrap();
    let result = engine.fire_timer("timer1").unwrap();
    assert_eq!(result.compat_state(), InstanceState::WaitingForTask);
    assert_eq!(
        result.active_element_ids().first().map(|s| s.as_str()),
        Some("task1")
    );
}

#[test]
fn new_instance_starts_at_start_event() {
    let def = parse_bpmn(simple_bpmn()).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();
    // Execution advances past StartEvent immediately to ServiceTask (which waits)
    assert_eq!(result.compat_state(), InstanceState::WaitingForTask);
    assert_eq!(
        result.active_element_ids().first().map(|s| s.as_str()),
        Some("ServiceTask_1")
    );
}

#[test]
fn completing_service_task_advances_to_end() {
    let def = parse_bpmn(simple_bpmn()).unwrap();
    let mut engine = Engine::new(def);
    engine.start(Default::default()).unwrap();
    let result = engine
        .complete_task("ServiceTask_1", Default::default())
        .unwrap();
    assert_eq!(result.compat_state(), InstanceState::Completed);
    assert!(result.active_element_ids().is_empty());
}

#[test]
fn variables_are_accessible_after_start() {
    let def = parse_bpmn(simple_bpmn()).unwrap();
    let mut engine = Engine::new(def);
    let vars = [("amount".to_string(), json!(100))].into_iter().collect();
    let result = engine.start(vars).unwrap();
    assert_eq!(result.variables.get("amount"), Some(&json!(100)));
}

#[test]
fn task_completion_merges_variables() {
    let def = parse_bpmn(simple_bpmn()).unwrap();
    let mut engine = Engine::new(def);
    engine.start(Default::default()).unwrap();
    let output = [("status".to_string(), json!("done"))]
        .into_iter()
        .collect();
    let result = engine.complete_task("ServiceTask_1", output).unwrap();
    assert_eq!(result.variables.get("status"), Some(&json!("done")));
}

const TIMER_START_EVENT_BPMN: &str = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="timer_start">
      <outgoing>sf1</outgoing>
      <timerEventDefinition><timeDuration>PT5M</timeDuration></timerEventDefinition>
    </startEvent>
    <sequenceFlow id="sf1" sourceRef="timer_start" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

#[test]
fn timer_start_event_process_advances_to_first_task() {
    let def = parse_bpmn(TIMER_START_EVENT_BPMN).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();
    // TimerStartEvent is treated like a regular StartEvent in the engine —
    // the scheduler is responsible for when to call start(); once called, execution
    // advances past the start event to the first task.
    assert_eq!(result.compat_state(), InstanceState::WaitingForTask);
    assert_eq!(
        result.active_element_ids().first().map(|s| s.as_str()),
        Some("task1")
    );
}

// ============================================================
// Expression evaluator tests
// ============================================================

mod expression_tests {
    use orrery::expression::eval;
    use serde_json::json;
    use std::collections::HashMap;

    fn vars(pairs: &[(&str, serde_json::Value)]) -> HashMap<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    // --- Existing == behaviour (regression) ---

    #[test]
    fn eq_bool_true() {
        assert!(eval(
            "${approved} == true",
            &vars(&[("approved", json!(true))])
        ));
    }

    #[test]
    fn eq_bool_false_value() {
        assert!(!eval(
            "${approved} == true",
            &vars(&[("approved", json!(false))])
        ));
    }

    #[test]
    fn eq_string() {
        assert!(eval(
            "${status} == 'active'",
            &vars(&[("status", json!("active"))])
        ));
        assert!(!eval(
            "${status} == 'active'",
            &vars(&[("status", json!("inactive"))])
        ));
    }

    #[test]
    fn eq_number() {
        assert!(eval("${amount} == 100", &vars(&[("amount", json!(100.0))])));
        // Integer variable vs integer literal (common from DB/JSON)
        assert!(eval("${task} == 2", &vars(&[("task", json!(2))])));
        assert!(!eval("${task} == 1", &vars(&[("task", json!(2))])));
        // Integer variable vs float literal
        assert!(eval("${task} == 2.0", &vars(&[("task", json!(2))])));
    }

    // --- New comparison operators ---

    #[test]
    fn not_equal() {
        assert!(eval("${x} != 10", &vars(&[("x", json!(5.0))])));
        assert!(!eval("${x} != 10", &vars(&[("x", json!(10.0))])));
    }

    #[test]
    fn greater_than() {
        assert!(eval(
            "${amount} > 1000",
            &vars(&[("amount", json!(1001.0))])
        ));
        assert!(!eval(
            "${amount} > 1000",
            &vars(&[("amount", json!(1000.0))])
        ));
        assert!(!eval(
            "${amount} > 1000",
            &vars(&[("amount", json!(999.0))])
        ));
    }

    #[test]
    fn less_than() {
        assert!(eval("${score} < 50", &vars(&[("score", json!(49.0))])));
        assert!(!eval("${score} < 50", &vars(&[("score", json!(50.0))])));
    }

    #[test]
    fn greater_than_or_equal() {
        assert!(eval("${score} >= 50", &vars(&[("score", json!(50.0))])));
        assert!(eval("${score} >= 50", &vars(&[("score", json!(51.0))])));
        assert!(!eval("${score} >= 50", &vars(&[("score", json!(49.0))])));
    }

    #[test]
    fn less_than_or_equal() {
        assert!(eval("${score} <= 100", &vars(&[("score", json!(100.0))])));
        assert!(eval("${score} <= 100", &vars(&[("score", json!(99.0))])));
        assert!(!eval("${score} <= 100", &vars(&[("score", json!(101.0))])));
    }

    // --- Boolean logic ---

    #[test]
    fn boolean_and() {
        let v = vars(&[("approved", json!(true)), ("amount", json!(500.0))]);
        assert!(eval("${approved} == true && ${amount} < 1000", &v));
        assert!(!eval("${approved} == true && ${amount} > 1000", &v));
    }

    #[test]
    fn boolean_or() {
        let v = vars(&[("status", json!("pending"))]);
        assert!(eval("${status} == 'pending' || ${status} == 'active'", &v));
        assert!(!eval("${status} == 'done' || ${status} == 'active'", &v));
    }

    #[test]
    fn boolean_not() {
        let v = vars(&[("cancelled", json!(false))]);
        assert!(eval("!${cancelled}", &v));
        let v2 = vars(&[("cancelled", json!(true))]);
        assert!(!eval("!${cancelled}", &v2));
    }

    // --- Mixed and parenthesised ---

    #[test]
    fn mixed_and_or() {
        let v = vars(&[("a", json!(15.0)), ("b", json!("x"))]);
        assert!(eval("${a} > 10 && ${b} == 'x'", &v));
        assert!(!eval("${a} > 10 && ${b} == 'y'", &v));
    }

    #[test]
    fn parenthesised_groups() {
        let v = vars(&[("a", json!(1.0)), ("b", json!(3.0)), ("c", json!(true))]);
        // (a > 0 && b < 5) || c == false  → true || false → true
        assert!(eval("(${a} > 0 && ${b} < 5) || ${c} == false", &v));
    }

    // --- Error cases ---

    #[test]
    fn undefined_variable_returns_false() {
        assert!(!eval("${does_not_exist} == true", &vars(&[])));
    }

    #[test]
    fn undefined_variable_in_comparison_returns_false() {
        assert!(!eval("${missing} > 10", &vars(&[])));
    }
}

#[test]
fn script_task_executes_and_stores_result() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:zeebe="http://camunda.org/schema/zeebe/1.0">
  <process id="Process_1" isExecutable="true">
    <startEvent id="Start_1">
      <outgoing>Flow_1</outgoing>
    </startEvent>
    <scriptTask id="Script_1" scriptFormat="rhai">
      <incoming>Flow_1</incoming>
      <outgoing>Flow_2</outgoing>
      <script>a + b</script>
      <extensionElements>
        <zeebe:script resultVariable="sum" />
      </extensionElements>
    </scriptTask>
    <endEvent id="End_1">
      <incoming>Flow_2</incoming>
    </endEvent>
    <sequenceFlow id="Flow_1" sourceRef="Start_1" targetRef="Script_1" />
    <sequenceFlow id="Flow_2" sourceRef="Script_1" targetRef="End_1" />
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let mut vars = std::collections::HashMap::new();
    vars.insert("a".to_string(), json!(10));
    vars.insert("b".to_string(), json!(20));
    let result = engine.start(vars).unwrap();
    assert!(result.is_completed);
    assert_eq!(result.variables.get("sum"), Some(&json!(30)));
}

// TODO: scope merge test disabled — modified_variables are not propagated back
// into process variables until we define clear semantics for this.
#[test]
fn script_task_does_not_leak_locals() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="Process_1" isExecutable="true">
    <startEvent id="Start_1">
      <outgoing>Flow_1</outgoing>
    </startEvent>
    <scriptTask id="Script_1" scriptFormat="rhai">
      <incoming>Flow_1</incoming>
      <outgoing>Flow_2</outgoing>
      <script>let y = x * 2; x += 1;</script>
    </scriptTask>
    <endEvent id="End_1">
      <incoming>Flow_2</incoming>
    </endEvent>
    <sequenceFlow id="Flow_1" sourceRef="Start_1" targetRef="Script_1" />
    <sequenceFlow id="Flow_2" sourceRef="Script_1" targetRef="End_1" />
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let mut vars = std::collections::HashMap::new();
    vars.insert("x".to_string(), json!(5));
    let result = engine.start(vars).unwrap();
    assert!(result.is_completed);
    // Script-local variables should NOT leak into process variables
    assert_eq!(result.variables.get("y"), None);
    // Original variable should remain unchanged (no scope merge)
    assert_eq!(result.variables.get("x"), Some(&json!(5)));
}

#[test]
fn script_task_unsupported_language_fails() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="Process_1" isExecutable="true">
    <startEvent id="Start_1">
      <outgoing>Flow_1</outgoing>
    </startEvent>
    <scriptTask id="Script_1" scriptFormat="python">
      <incoming>Flow_1</incoming>
      <outgoing>Flow_2</outgoing>
      <script>x = 1</script>
    </scriptTask>
    <endEvent id="End_1">
      <incoming>Flow_2</incoming>
    </endEvent>
    <sequenceFlow id="Flow_1" sourceRef="Start_1" targetRef="Script_1" />
    <sequenceFlow id="Flow_2" sourceRef="Script_1" targetRef="End_1" />
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default());
    assert!(result.is_err());
}

#[test]
fn script_task_runtime_error_fails() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="Process_1" isExecutable="true">
    <startEvent id="Start_1">
      <outgoing>Flow_1</outgoing>
    </startEvent>
    <scriptTask id="Script_1" scriptFormat="rhai">
      <incoming>Flow_1</incoming>
      <outgoing>Flow_2</outgoing>
      <script>let x = 0; loop { x += 1; }</script>
    </scriptTask>
    <endEvent id="End_1">
      <incoming>Flow_2</incoming>
    </endEvent>
    <sequenceFlow id="Flow_1" sourceRef="Start_1" targetRef="Script_1" />
    <sequenceFlow id="Flow_2" sourceRef="Script_1" targetRef="End_1" />
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default());
    assert!(result.is_err());
}

#[test]
fn signal_start_event_advances_to_first_task() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <signal id="Sig_1" name="OrderPlaced"/>
  <process id="p">
    <startEvent id="sig_start">
      <signalEventDefinition signalRef="Sig_1"/>
      <outgoing>f1</outgoing>
    </startEvent>
    <sequenceFlow id="f1" sourceRef="sig_start" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();
    assert_eq!(result.compat_state(), InstanceState::WaitingForTask);
    assert_eq!(
        result.active_element_ids().first().map(|s| s.as_str()),
        Some("task1")
    );
}

#[test]
fn signal_intermediate_throw_passes_through() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <signal id="Sig_1" name="OrderShipped"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="throw1"/>
    <intermediateThrowEvent id="throw1">
      <signalEventDefinition signalRef="Sig_1"/>
      <outgoing>f2</outgoing>
    </intermediateThrowEvent>
    <sequenceFlow id="f2" sourceRef="throw1" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f3</outgoing></serviceTask>
    <sequenceFlow id="f3" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();
    // SignalIntermediateThrowEvent is pass-through — should advance to task1
    assert_eq!(result.compat_state(), InstanceState::WaitingForTask);
    assert_eq!(result.active_element_ids(), vec!["task1"]);
}

#[test]
fn signal_end_event_completes_instance() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <signal id="Sig_1" name="ProcessDone"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="sig_end"/>
    <endEvent id="sig_end">
      <signalEventDefinition signalRef="Sig_1"/>
    </endEvent>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();
    assert!(result.is_completed);
    assert!(result.active_elements.is_empty());
}

#[test]
fn receive_boundary_signal_interrupting_cancels_task() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <signal id="Sig_1" name="CancelOrder"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="end1"/>
    <endEvent id="end1"/>
    <boundaryEvent id="sig_bound" attachedToRef="task1" cancelActivity="true">
      <outgoing>f3</outgoing>
      <signalEventDefinition signalRef="Sig_1"/>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="sig_bound" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();
    assert_eq!(result.active_element_ids(), vec!["task1"]);

    // Fire boundary signal — should cancel task and complete via end2
    let result = engine
        .receive_boundary_signal("sig_bound", Default::default())
        .unwrap();
    assert!(result.is_completed);
    assert!(result.active_elements.is_empty());
}

#[test]
fn receive_boundary_signal_non_interrupting_keeps_task() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <signal id="Sig_1" name="Notification"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="end1"/>
    <endEvent id="end1"/>
    <boundaryEvent id="sig_bound" attachedToRef="task1" cancelActivity="false">
      <outgoing>f3</outgoing>
      <signalEventDefinition signalRef="Sig_1"/>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="sig_bound" targetRef="task2"/>
    <serviceTask id="task2"><outgoing>f4</outgoing></serviceTask>
    <sequenceFlow id="f4" sourceRef="task2" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();
    assert_eq!(result.active_element_ids(), vec!["task1"]);

    // Fire boundary signal non-interrupting — task1 stays, task2 also active
    let result = engine
        .receive_boundary_signal("sig_bound", Default::default())
        .unwrap();
    assert!(!result.is_completed);
    let ids = result.active_element_ids();
    assert!(ids.contains(&"task1".to_string()));
    assert!(ids.contains(&"task2".to_string()));
}

#[test]
fn fire_boundary_timer_interrupting_cancels_task() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="end1"/>
    <endEvent id="end1"/>
    <boundaryEvent id="timer_bound" attachedToRef="task1" cancelActivity="true">
      <outgoing>f3</outgoing>
      <timerEventDefinition><timeDuration>PT1H</timeDuration></timerEventDefinition>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="timer_bound" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();
    assert_eq!(result.active_element_ids(), vec!["task1"]);

    // Fire boundary timer — should cancel task and complete via end2
    let result = engine.fire_boundary_timer("timer_bound").unwrap();
    assert!(result.is_completed);
    assert!(result.active_elements.is_empty());
}

#[test]
fn fire_boundary_timer_non_interrupting_keeps_task() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="end1"/>
    <endEvent id="end1"/>
    <boundaryEvent id="timer_bound" attachedToRef="task1" cancelActivity="false">
      <outgoing>f3</outgoing>
      <timerEventDefinition><timeDuration>PT30M</timeDuration></timerEventDefinition>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="timer_bound" targetRef="task2"/>
    <serviceTask id="task2"><outgoing>f4</outgoing></serviceTask>
    <sequenceFlow id="f4" sourceRef="task2" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();
    assert_eq!(result.active_element_ids(), vec!["task1"]);

    // Fire boundary timer non-interrupting — task1 stays, task2 also active
    let result = engine.fire_boundary_timer("timer_bound").unwrap();
    assert!(!result.is_completed);
    let ids = result.active_element_ids();
    assert!(ids.contains(&"task1".to_string()));
    assert!(ids.contains(&"task2".to_string()));
}

#[test]
fn error_end_event_caught_by_matching_boundary() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <error id="Err_1" name="PaymentFailed" errorCode="PAYMENT_ERR"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="sub"/>
    <subProcess id="sub">
      <startEvent id="sub_s"><outgoing>sf1</outgoing></startEvent>
      <sequenceFlow id="sf1" sourceRef="sub_s" targetRef="err_end"/>
      <endEvent id="err_end">
        <errorEventDefinition errorRef="Err_1"/>
      </endEvent>
      <outgoing>f2</outgoing>
    </subProcess>
    <sequenceFlow id="f2" sourceRef="sub" targetRef="end1"/>
    <endEvent id="end1"/>
    <boundaryEvent id="err_bound" attachedToRef="sub">
      <outgoing>f3</outgoing>
      <errorEventDefinition errorRef="Err_1"/>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="err_bound" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();

    assert!(result.is_completed);
    assert!(!result.is_failed);
}

#[test]
fn error_end_event_unmatched_code_skips_boundary() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <error id="Err_1" name="PaymentFailed" errorCode="PAYMENT_ERR"/>
  <error id="Err_2" name="Timeout" errorCode="TIMEOUT"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="sub"/>
    <subProcess id="sub">
      <startEvent id="sub_s"><outgoing>sf1</outgoing></startEvent>
      <sequenceFlow id="sf1" sourceRef="sub_s" targetRef="err_end"/>
      <endEvent id="err_end">
        <errorEventDefinition errorRef="Err_1"/>
      </endEvent>
      <outgoing>f2</outgoing>
    </subProcess>
    <sequenceFlow id="f2" sourceRef="sub" targetRef="end1"/>
    <endEvent id="end1"/>
    <boundaryEvent id="err_bound" attachedToRef="sub">
      <outgoing>f3</outgoing>
      <errorEventDefinition errorRef="Err_2"/>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="err_bound" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();

    assert!(result.is_failed);
}

#[test]
fn error_catchall_boundary_catches_any_error() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <error id="Err_1" name="PaymentFailed" errorCode="PAYMENT_ERR"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="sub"/>
    <subProcess id="sub">
      <startEvent id="sub_s"><outgoing>sf1</outgoing></startEvent>
      <sequenceFlow id="sf1" sourceRef="sub_s" targetRef="err_end"/>
      <endEvent id="err_end">
        <errorEventDefinition errorRef="Err_1"/>
      </endEvent>
      <outgoing>f2</outgoing>
    </subProcess>
    <sequenceFlow id="f2" sourceRef="sub" targetRef="end1"/>
    <endEvent id="end1"/>
    <boundaryEvent id="err_bound" attachedToRef="sub">
      <outgoing>f3</outgoing>
      <errorEventDefinition/>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="err_bound" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();

    assert!(result.is_completed);
    assert!(!result.is_failed);
}

#[test]
fn terminate_end_kills_all_parallel_branches() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="fork"/>
    <parallelGateway id="fork"><incoming>f1</incoming><outgoing>f2</outgoing><outgoing>f3</outgoing></parallelGateway>
    <sequenceFlow id="f2" sourceRef="fork" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f4</outgoing></serviceTask>
    <sequenceFlow id="f4" sourceRef="task1" targetRef="end1"/>
    <endEvent id="end1"/>
    <sequenceFlow id="f3" sourceRef="fork" targetRef="term"/>
    <endEvent id="term">
      <terminateEventDefinition/>
    </endEvent>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();

    // Terminate end should kill task1's branch — instance completed immediately
    assert!(result.is_completed);
    assert!(result.active_elements.is_empty());
}

#[test]
fn terminate_end_simple_linear_completes() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="term"/>
    <endEvent id="term">
      <terminateEventDefinition/>
    </endEvent>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();

    assert!(result.is_completed);
    assert!(result.active_elements.is_empty());
}

#[test]
fn terminate_end_in_subprocess_completes_subprocess_only() {
    // TerminateEndEvent inside a subprocess should complete the subprocess,
    // then the parent continues past it
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="sub"/>
    <subProcess id="sub">
      <startEvent id="sub_s"><outgoing>sf1</outgoing></startEvent>
      <sequenceFlow id="sf1" sourceRef="sub_s" targetRef="sub_term"/>
      <endEvent id="sub_term"><terminateEventDefinition/></endEvent>
      <outgoing>f2</outgoing>
    </subProcess>
    <sequenceFlow id="f2" sourceRef="sub" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f3</outgoing></serviceTask>
    <sequenceFlow id="f3" sourceRef="task1" targetRef="end1"/>
    <endEvent id="end1"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();

    // Subprocess terminates, parent continues to task1
    assert!(
        !result.is_completed,
        "Parent should NOT be completed — task1 still waiting"
    );
    assert_eq!(result.active_elements.len(), 1);
    assert_eq!(result.active_elements[0].element_id, "task1");
}

#[test]
fn escalation_throw_passes_through() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <escalation id="esc1" escalationCode="ESC_001"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="esc_throw"/>
    <intermediateThrowEvent id="esc_throw">
      <escalationEventDefinition escalationRef="esc1"/>
      <outgoing>f2</outgoing>
    </intermediateThrowEvent>
    <sequenceFlow id="f2" sourceRef="esc_throw" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f3</outgoing></serviceTask>
    <sequenceFlow id="f3" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();

    // Escalation throw is pass-through — should advance to task1
    assert_eq!(result.active_element_ids(), vec!["task1"]);
    assert_eq!(result.thrown_escalation.as_deref(), Some("ESC_001"));
}

#[test]
fn escalation_end_completes_with_code() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <escalation id="esc1" escalationCode="ESC_002"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="esc_end"/>
    <endEvent id="esc_end">
      <escalationEventDefinition escalationRef="esc1"/>
    </endEvent>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();

    assert!(result.is_completed);
    assert_eq!(result.thrown_escalation.as_deref(), Some("ESC_002"));
}

#[test]
fn escalation_boundary_catches_matching_code() {
    // Subprocess throws escalation ESC_003 → interrupting boundary catches it
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <escalation id="esc1" escalationCode="ESC_003"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="sub"/>
    <subProcess id="sub"><outgoing>f2</outgoing>
      <startEvent id="sub_s"><outgoing>sf1</outgoing></startEvent>
      <sequenceFlow id="sf1" sourceRef="sub_s" targetRef="sub_esc_end"/>
      <endEvent id="sub_esc_end">
        <escalationEventDefinition escalationRef="esc1"/>
      </endEvent>
    </subProcess>
    <sequenceFlow id="f2" sourceRef="sub" targetRef="normal_end"/>
    <endEvent id="normal_end"/>
    <boundaryEvent id="esc_bound" attachedToRef="sub" cancelActivity="true">
      <outgoing>f3</outgoing>
      <escalationEventDefinition escalationRef="esc1"/>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="esc_bound" targetRef="esc_task"/>
    <serviceTask id="esc_task"><outgoing>f4</outgoing></serviceTask>
    <sequenceFlow id="f4" sourceRef="esc_task" targetRef="esc_end"/>
    <endEvent id="esc_end"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();

    // Interrupting boundary catches escalation → routes to esc_task
    assert_eq!(result.active_element_ids(), vec!["esc_task"]);
}

#[test]
fn escalation_boundary_no_match_continues_normally() {
    // Subprocess throws escalation ESC_A but boundary catches ESC_B → no match, continues
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <escalation id="esc_a" escalationCode="ESC_A"/>
  <escalation id="esc_b" escalationCode="ESC_B"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="sub"/>
    <subProcess id="sub"><outgoing>f2</outgoing>
      <startEvent id="sub_s"><outgoing>sf1</outgoing></startEvent>
      <sequenceFlow id="sf1" sourceRef="sub_s" targetRef="sub_esc_end"/>
      <endEvent id="sub_esc_end">
        <escalationEventDefinition escalationRef="esc_a"/>
      </endEvent>
    </subProcess>
    <sequenceFlow id="f2" sourceRef="sub" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f3</outgoing></serviceTask>
    <sequenceFlow id="f3" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
    <boundaryEvent id="esc_bound" attachedToRef="sub" cancelActivity="true">
      <outgoing>f4</outgoing>
      <escalationEventDefinition escalationRef="esc_b"/>
    </boundaryEvent>
    <sequenceFlow id="f4" sourceRef="esc_bound" targetRef="esc_task"/>
    <serviceTask id="esc_task"><outgoing>f5</outgoing></serviceTask>
    <sequenceFlow id="f5" sourceRef="esc_task" targetRef="esc_end"/>
    <endEvent id="esc_end"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();

    // No match → subprocess completes normally → parent advances to task1
    assert_eq!(result.active_element_ids(), vec!["task1"]);
}

#[test]
fn escalation_catch_all_boundary() {
    // Boundary with no escalation code catches any escalation
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <escalation id="esc1" escalationCode="ANY_CODE"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="sub"/>
    <subProcess id="sub"><outgoing>f2</outgoing>
      <startEvent id="sub_s"><outgoing>sf1</outgoing></startEvent>
      <sequenceFlow id="sf1" sourceRef="sub_s" targetRef="sub_esc_end"/>
      <endEvent id="sub_esc_end">
        <escalationEventDefinition escalationRef="esc1"/>
      </endEvent>
    </subProcess>
    <sequenceFlow id="f2" sourceRef="sub" targetRef="normal_end"/>
    <endEvent id="normal_end"/>
    <boundaryEvent id="esc_bound" attachedToRef="sub" cancelActivity="true">
      <outgoing>f3</outgoing>
      <escalationEventDefinition/>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="esc_bound" targetRef="caught_task"/>
    <serviceTask id="caught_task"><outgoing>f4</outgoing></serviceTask>
    <sequenceFlow id="f4" sourceRef="caught_task" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();

    // Catch-all boundary catches the escalation → routes to caught_task
    assert_eq!(result.active_element_ids(), vec!["caught_task"]);
}

#[test]
fn link_throw_jumps_to_matching_catch() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="link_throw"/>
    <intermediateThrowEvent id="link_throw">
      <linkEventDefinition name="jump1"/>
    </intermediateThrowEvent>
    <intermediateCatchEvent id="link_catch">
      <linkEventDefinition name="jump1"/>
      <outgoing>f2</outgoing>
    </intermediateCatchEvent>
    <sequenceFlow id="f2" sourceRef="link_catch" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f3</outgoing></serviceTask>
    <sequenceFlow id="f3" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();

    // Should jump from link_throw → link_catch → task1
    assert_eq!(result.active_element_ids(), vec!["task1"]);
}

#[test]
fn multiple_link_throws_to_same_catch() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="gw"/>
    <exclusiveGateway id="gw" default="f3"><incoming>f1</incoming><outgoing>f2</outgoing><outgoing>f3</outgoing></exclusiveGateway>
    <sequenceFlow id="f2" sourceRef="gw" targetRef="link_throw_a">
      <conditionExpression>${go_a == true}</conditionExpression>
    </sequenceFlow>
    <intermediateThrowEvent id="link_throw_a"><linkEventDefinition name="target"/></intermediateThrowEvent>
    <sequenceFlow id="f3" sourceRef="gw" targetRef="link_throw_b">
    </sequenceFlow>
    <intermediateThrowEvent id="link_throw_b"><linkEventDefinition name="target"/></intermediateThrowEvent>
    <intermediateCatchEvent id="link_catch">
      <linkEventDefinition name="target"/>
      <outgoing>f4</outgoing>
    </intermediateCatchEvent>
    <sequenceFlow id="f4" sourceRef="link_catch" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f5</outgoing></serviceTask>
    <sequenceFlow id="f5" sourceRef="task1" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let mut vars = HashMap::new();
    vars.insert("go_a".to_string(), serde_json::json!(true));
    let result = engine.start(vars).unwrap();

    // go_a is true → link_throw_a → link_catch → task1
    assert_eq!(result.active_element_ids(), vec!["task1"]);
}

#[test]
fn escalation_non_interrupting_boundary_spawns_parallel_path() {
    // Subprocess throws escalation → non-interrupting boundary catches it
    // Both: subprocess normal flow continues AND boundary path spawns
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <escalation id="esc1" escalationCode="ESC_NI"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="sub"/>
    <subProcess id="sub"><outgoing>f2</outgoing>
      <startEvent id="sub_s"><outgoing>sf1</outgoing></startEvent>
      <sequenceFlow id="sf1" sourceRef="sub_s" targetRef="sub_esc_end"/>
      <endEvent id="sub_esc_end">
        <escalationEventDefinition escalationRef="esc1"/>
      </endEvent>
    </subProcess>
    <sequenceFlow id="f2" sourceRef="sub" targetRef="normal_task"/>
    <serviceTask id="normal_task"><outgoing>f3</outgoing></serviceTask>
    <sequenceFlow id="f3" sourceRef="normal_task" targetRef="end"/>
    <endEvent id="end"/>
    <boundaryEvent id="esc_bound" attachedToRef="sub" cancelActivity="false">
      <outgoing>f4</outgoing>
      <escalationEventDefinition escalationRef="esc1"/>
    </boundaryEvent>
    <sequenceFlow id="f4" sourceRef="esc_bound" targetRef="esc_task"/>
    <serviceTask id="esc_task"><outgoing>f5</outgoing></serviceTask>
    <sequenceFlow id="f5" sourceRef="esc_task" targetRef="esc_end"/>
    <endEvent id="esc_end"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();

    // Non-interrupting: both normal flow (normal_task) AND boundary path (esc_task)
    let mut ids = result.active_element_ids();
    ids.sort();
    assert_eq!(ids, vec!["esc_task", "normal_task"]);
}

#[test]
fn escalation_specific_code_wins_over_catch_all() {
    // Two boundaries: one specific (ESC_X), one catch-all. Thrown code is ESC_X.
    // Specific should win.
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <escalation id="esc_x" escalationCode="ESC_X"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="sub"/>
    <subProcess id="sub"><outgoing>f2</outgoing>
      <startEvent id="sub_s"><outgoing>sf1</outgoing></startEvent>
      <sequenceFlow id="sf1" sourceRef="sub_s" targetRef="sub_esc_end"/>
      <endEvent id="sub_esc_end">
        <escalationEventDefinition escalationRef="esc_x"/>
      </endEvent>
    </subProcess>
    <sequenceFlow id="f2" sourceRef="sub" targetRef="normal_end"/>
    <endEvent id="normal_end"/>
    <boundaryEvent id="specific_bound" attachedToRef="sub" cancelActivity="true">
      <outgoing>f3</outgoing>
      <escalationEventDefinition escalationRef="esc_x"/>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="specific_bound" targetRef="specific_task"/>
    <serviceTask id="specific_task"><outgoing>f4</outgoing></serviceTask>
    <sequenceFlow id="f4" sourceRef="specific_task" targetRef="end1"/>
    <endEvent id="end1"/>
    <boundaryEvent id="catchall_bound" attachedToRef="sub" cancelActivity="true">
      <outgoing>f5</outgoing>
      <escalationEventDefinition/>
    </boundaryEvent>
    <sequenceFlow id="f5" sourceRef="catchall_bound" targetRef="catchall_task"/>
    <serviceTask id="catchall_task"><outgoing>f6</outgoing></serviceTask>
    <sequenceFlow id="f6" sourceRef="catchall_task" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default()).unwrap();

    // Specific code match wins → routes to specific_task, NOT catchall_task
    assert_eq!(result.active_element_ids(), vec!["specific_task"]);
}

#[test]
fn link_throw_with_no_matching_catch_errors() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="link_throw"/>
    <intermediateThrowEvent id="link_throw">
      <linkEventDefinition name="nonexistent"/>
    </intermediateThrowEvent>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(Default::default());

    assert!(
        result.is_err(),
        "Should error when no matching link catch exists"
    );
    match result {
        Err(EngineError::TargetNotFound(msg)) => {
            assert!(
                msg.contains("nonexistent"),
                "Error should mention the link name: {msg}"
            );
        }
        other => panic!("Expected TargetNotFound error, got {:?}", other),
    }
}

// =========================================================================
// Visit tracking tests
// =========================================================================

use orrery::model::VisitEvent;

/// Helper: extract (element_id, element_type, event_variant) triples from visited.
fn visit_summary(result: &orrery::engine::ExecutionResult) -> Vec<(&str, &str, &str)> {
    result
        .visited
        .iter()
        .map(|v| {
            let event = match &v.event {
                VisitEvent::Activated => "Activated",
                VisitEvent::Completed => "Completed",
                VisitEvent::ErrorThrown => "ErrorThrown",
                VisitEvent::EscalationThrown => "EscalationThrown",
                VisitEvent::MessageThrown => "MessageThrown",
                VisitEvent::LinkJumped => "LinkJumped",
                VisitEvent::Terminated => "Terminated",
            };
            (v.element_id.as_str(), v.element_type.as_str(), event)
        })
        .collect()
}

#[test]
fn visited_tracks_pass_through_elements() {
    let bpmn = r#"<?xml version="1.0"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
      <process id="p" isExecutable="true">
        <startEvent id="start"><outgoing>f1</outgoing></startEvent>
        <scriptTask id="script" scriptFormat="rhai">
          <incoming>f1</incoming><outgoing>f2</outgoing>
          <script>1 + 1</script>
        </scriptTask>
        <endEvent id="end"><incoming>f2</incoming></endEvent>
        <sequenceFlow id="f1" sourceRef="start" targetRef="script"/>
        <sequenceFlow id="f2" sourceRef="script" targetRef="end"/>
      </process>
    </definitions>"#;
    let def = parse_bpmn(bpmn).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(HashMap::new()).unwrap();

    assert!(result.is_completed);
    let summary = visit_summary(&result);

    // Start: Activated + Completed
    assert!(summary.contains(&("start", "StartEvent", "Activated")));
    assert!(summary.contains(&("start", "StartEvent", "Completed")));
    // Script: Activated + Completed
    assert!(summary.contains(&("script", "ScriptTask", "Activated")));
    assert!(summary.contains(&("script", "ScriptTask", "Completed")));
    // End: Activated + Completed
    assert!(summary.contains(&("end", "EndEvent", "Activated")));
    assert!(summary.contains(&("end", "EndEvent", "Completed")));

    // Verify ordering: start before script before end
    let ids: Vec<&str> = result
        .visited
        .iter()
        .map(|v| v.element_id.as_str())
        .collect();
    let start_pos = ids.iter().position(|&id| id == "start").unwrap();
    let script_pos = ids.iter().position(|&id| id == "script").unwrap();
    let end_pos = ids.iter().position(|&id| id == "end").unwrap();
    assert!(start_pos < script_pos);
    assert!(script_pos < end_pos);
}

#[test]
fn visited_records_activated_for_wait_state() {
    let bpmn = r#"<?xml version="1.0"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
      <process id="p" isExecutable="true">
        <startEvent id="start"><outgoing>f1</outgoing></startEvent>
        <serviceTask id="task"><outgoing>f2</outgoing></serviceTask>
        <endEvent id="end"><incoming>f2</incoming></endEvent>
        <sequenceFlow id="f1" sourceRef="start" targetRef="task"/>
        <sequenceFlow id="f2" sourceRef="task" targetRef="end"/>
      </process>
    </definitions>"#;
    let def = parse_bpmn(bpmn).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(HashMap::new()).unwrap();

    assert!(!result.is_completed);
    let summary = visit_summary(&result);

    // ServiceTask should only have Activated (no Completed yet)
    let task_events: Vec<&str> = summary
        .iter()
        .filter(|(id, _, _)| *id == "task")
        .map(|(_, _, ev)| *ev)
        .collect();
    assert_eq!(task_events, vec!["Activated"]);
}

#[test]
fn visited_complete_task_records_completed_then_subsequent() {
    let bpmn = r#"<?xml version="1.0"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
      <process id="p" isExecutable="true">
        <startEvent id="start"><outgoing>f1</outgoing></startEvent>
        <serviceTask id="task"><outgoing>f2</outgoing></serviceTask>
        <endEvent id="end"><incoming>f2</incoming></endEvent>
        <sequenceFlow id="f1" sourceRef="start" targetRef="task"/>
        <sequenceFlow id="f2" sourceRef="task" targetRef="end"/>
      </process>
    </definitions>"#;
    let def = parse_bpmn(bpmn).unwrap();
    let mut engine = Engine::new(def);
    engine.start(HashMap::new()).unwrap();

    // Complete the task — should record Completed for task + traversal of end
    let result = engine.complete_task("task", HashMap::new()).unwrap();
    assert!(result.is_completed);
    let summary = visit_summary(&result);

    // Task Completed (from complete_task)
    assert!(summary.contains(&("task", "ServiceTask", "Completed")));
    // End: Activated + Completed (from advance_from)
    assert!(summary.contains(&("end", "EndEvent", "Activated")));
    assert!(summary.contains(&("end", "EndEvent", "Completed")));
}

#[test]
fn visited_exclusive_gateway_appears_in_history() {
    let bpmn = r#"<?xml version="1.0"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
      <process id="p" isExecutable="true">
        <startEvent id="start"><outgoing>f1</outgoing></startEvent>
        <exclusiveGateway id="gw" default="f3">
          <outgoing>f2</outgoing>
          <outgoing>f3</outgoing>
        </exclusiveGateway>
        <serviceTask id="task_a"><outgoing>f4</outgoing></serviceTask>
        <serviceTask id="task_b"><outgoing>f5</outgoing></serviceTask>
        <endEvent id="end"/>
        <sequenceFlow id="f1" sourceRef="start" targetRef="gw"/>
        <sequenceFlow id="f2" sourceRef="gw" targetRef="task_a">
          <conditionExpression>x > 10</conditionExpression>
        </sequenceFlow>
        <sequenceFlow id="f3" sourceRef="gw" targetRef="task_b"/>
        <sequenceFlow id="f4" sourceRef="task_a" targetRef="end"/>
        <sequenceFlow id="f5" sourceRef="task_b" targetRef="end"/>
      </process>
    </definitions>"#;
    let def = parse_bpmn(bpmn).unwrap();
    let mut engine = Engine::new(def);
    let mut vars = HashMap::new();
    vars.insert("x".to_string(), json!(5));
    let result = engine.start(vars).unwrap();

    let summary = visit_summary(&result);

    // Gateway should have Activated + Completed
    assert!(summary.contains(&("gw", "ExclusiveGateway", "Activated")));
    assert!(summary.contains(&("gw", "ExclusiveGateway", "Completed")));
    // Should route to task_b (default) since x=5 < 10
    assert!(summary.contains(&("task_b", "ServiceTask", "Activated")));
    // task_a should NOT appear
    assert!(!summary.iter().any(|(id, _, _)| *id == "task_a"));
}

#[test]
fn visited_link_jump_records_link_jumped_event() {
    let bpmn = r#"<?xml version="1.0"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
      <process id="p" isExecutable="true">
        <startEvent id="start"><outgoing>f1</outgoing></startEvent>
        <intermediateThrowEvent id="link_throw">
          <outgoing>f_dead</outgoing>
          <linkEventDefinition name="jump1"/>
        </intermediateThrowEvent>
        <intermediateCatchEvent id="link_catch">
          <outgoing>f3</outgoing>
          <linkEventDefinition name="jump1"/>
        </intermediateCatchEvent>
        <endEvent id="end"><incoming>f3</incoming></endEvent>
        <sequenceFlow id="f1" sourceRef="start" targetRef="link_throw"/>
        <sequenceFlow id="f_dead" sourceRef="link_throw" targetRef="end"/>
        <sequenceFlow id="f3" sourceRef="link_catch" targetRef="end"/>
      </process>
    </definitions>"#;
    let def = parse_bpmn(bpmn).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(HashMap::new()).unwrap();

    assert!(result.is_completed);
    let summary = visit_summary(&result);

    // Link throw: Activated + LinkJumped
    assert!(summary.contains(&("link_throw", "LinkIntermediateThrowEvent", "Activated")));
    assert!(summary.contains(&("link_throw", "LinkIntermediateThrowEvent", "LinkJumped")));
    // Link catch: Activated + Completed
    assert!(summary.contains(&("link_catch", "LinkIntermediateCatchEvent", "Activated")));
    assert!(summary.contains(&("link_catch", "LinkIntermediateCatchEvent", "Completed")));
}

#[test]
fn visited_error_end_records_error_thrown() {
    let bpmn = r#"<?xml version="1.0"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
      <error id="err1" errorCode="VALIDATION_FAILED"/>
      <process id="p" isExecutable="true">
        <startEvent id="start"><outgoing>f1</outgoing></startEvent>
        <endEvent id="error_end">
          <errorEventDefinition errorRef="err1"/>
        </endEvent>
        <sequenceFlow id="f1" sourceRef="start" targetRef="error_end"/>
      </process>
    </definitions>"#;
    let def = parse_bpmn(bpmn).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(HashMap::new()).unwrap();

    assert!(result.is_failed);
    let summary = visit_summary(&result);
    assert!(summary.contains(&("error_end", "ErrorEndEvent", "Activated")));
    assert!(summary.contains(&("error_end", "ErrorEndEvent", "ErrorThrown")));
}

#[test]
fn visited_terminate_end_records_terminated() {
    let bpmn = r#"<?xml version="1.0"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
      <process id="p" isExecutable="true">
        <startEvent id="start"><outgoing>f1</outgoing></startEvent>
        <endEvent id="term_end">
          <terminateEventDefinition/>
        </endEvent>
        <sequenceFlow id="f1" sourceRef="start" targetRef="term_end"/>
      </process>
    </definitions>"#;
    let def = parse_bpmn(bpmn).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(HashMap::new()).unwrap();

    assert!(result.is_completed);
    let summary = visit_summary(&result);
    assert!(summary.contains(&("term_end", "TerminateEndEvent", "Activated")));
    assert!(summary.contains(&("term_end", "TerminateEndEvent", "Terminated")));
}

#[test]
fn visited_parallel_gateway_fork_and_join() {
    let bpmn = r#"<?xml version="1.0"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
      <process id="p" isExecutable="true">
        <startEvent id="start"><outgoing>f0</outgoing></startEvent>
        <parallelGateway id="fork">
          <incoming>f0</incoming>
          <outgoing>fa</outgoing>
          <outgoing>fb</outgoing>
        </parallelGateway>
        <serviceTask id="ta"><incoming>fa</incoming><outgoing>fca</outgoing></serviceTask>
        <serviceTask id="tb"><incoming>fb</incoming><outgoing>fcb</outgoing></serviceTask>
        <parallelGateway id="join">
          <incoming>fca</incoming>
          <incoming>fcb</incoming>
          <outgoing>fe</outgoing>
        </parallelGateway>
        <endEvent id="end"><incoming>fe</incoming></endEvent>
        <sequenceFlow id="f0" sourceRef="start" targetRef="fork"/>
        <sequenceFlow id="fa" sourceRef="fork" targetRef="ta"/>
        <sequenceFlow id="fb" sourceRef="fork" targetRef="tb"/>
        <sequenceFlow id="fca" sourceRef="ta" targetRef="join"/>
        <sequenceFlow id="fcb" sourceRef="tb" targetRef="join"/>
        <sequenceFlow id="fe" sourceRef="join" targetRef="end"/>
      </process>
    </definitions>"#;
    let def = parse_bpmn(bpmn).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(HashMap::new()).unwrap();

    let summary = visit_summary(&result);

    // Fork gateway: Activated + Completed
    assert!(summary.contains(&("fork", "ParallelGateway", "Activated")));
    assert!(summary.contains(&("fork", "ParallelGateway", "Completed")));
    // Both branches activated
    assert!(summary.contains(&("ta", "ServiceTask", "Activated")));
    assert!(summary.contains(&("tb", "ServiceTask", "Activated")));
    // Join is NOT reached yet during start() — branches stop at service tasks

    // Complete first branch — join receives one token, waits for second
    let r1 = engine.complete_task("ta", HashMap::new()).unwrap();
    let summary1 = visit_summary(&r1);
    // Join should be Activated (first token arrives) but NOT Completed (still waiting)
    assert!(summary1.contains(&("join", "ParallelGateway", "Activated")));
    assert!(
        !summary1.contains(&("join", "ParallelGateway", "Completed")),
        "Join should not be completed with only one branch done"
    );

    // Complete second branch — join fires
    let result2 = engine.complete_task("tb", HashMap::new()).unwrap();
    assert!(result2.is_completed);

    let summary2 = visit_summary(&result2);
    // After second branch arrives, join should fire: Activated + Completed
    assert!(summary2.contains(&("join", "ParallelGateway", "Activated")));
    assert!(summary2.contains(&("join", "ParallelGateway", "Completed")));
    assert!(summary2.contains(&("end", "EndEvent", "Activated")));
    assert!(summary2.contains(&("end", "EndEvent", "Completed")));
}

#[test]
fn visited_fire_timer_records_completed() {
    let bpmn = r#"<?xml version="1.0"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
      <process id="p" isExecutable="true">
        <startEvent id="start"><outgoing>f1</outgoing></startEvent>
        <intermediateCatchEvent id="timer1">
          <outgoing>f2</outgoing>
          <timerEventDefinition><timeDuration>PT5M</timeDuration></timerEventDefinition>
        </intermediateCatchEvent>
        <endEvent id="end"><incoming>f2</incoming></endEvent>
        <sequenceFlow id="f1" sourceRef="start" targetRef="timer1"/>
        <sequenceFlow id="f2" sourceRef="timer1" targetRef="end"/>
      </process>
    </definitions>"#;
    let def = parse_bpmn(bpmn).unwrap();
    let mut engine = Engine::new(def);
    let start_result = engine.start(HashMap::new()).unwrap();

    // Timer should be activated
    let summary = visit_summary(&start_result);
    assert!(summary.contains(&("timer1", "TimerIntermediateEvent", "Activated")));

    // Fire the timer
    let result = engine.fire_timer("timer1").unwrap();
    assert!(result.is_completed);
    let summary2 = visit_summary(&result);
    assert!(summary2.contains(&("timer1", "TimerIntermediateEvent", "Completed")));
    assert!(summary2.contains(&("end", "EndEvent", "Activated")));
}

#[test]
fn visited_fail_task_records_error_thrown() {
    let bpmn = r#"<?xml version="1.0"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
      <process id="p" isExecutable="true">
        <startEvent id="start"><outgoing>f1</outgoing></startEvent>
        <serviceTask id="task"><outgoing>f2</outgoing></serviceTask>
        <endEvent id="end"><incoming>f2</incoming></endEvent>
        <sequenceFlow id="f1" sourceRef="start" targetRef="task"/>
        <sequenceFlow id="f2" sourceRef="task" targetRef="end"/>
      </process>
    </definitions>"#;
    let def = parse_bpmn(bpmn).unwrap();
    let mut engine = Engine::new(def);
    engine.start(HashMap::new()).unwrap();

    let result = engine.fail_task("task", None).unwrap();
    assert!(result.is_failed);
    let summary = visit_summary(&result);
    assert!(summary.contains(&("task", "ServiceTask", "ErrorThrown")));
}

#[test]
fn visited_message_throw_records_message_thrown() {
    let bpmn = r#"<?xml version="1.0"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
      <message id="msg1" name="notify"/>
      <process id="p" isExecutable="true">
        <startEvent id="start"><outgoing>f1</outgoing></startEvent>
        <intermediateThrowEvent id="msg_throw">
          <outgoing>f2</outgoing>
          <messageEventDefinition messageRef="msg1"/>
        </intermediateThrowEvent>
        <endEvent id="end"><incoming>f2</incoming></endEvent>
        <sequenceFlow id="f1" sourceRef="start" targetRef="msg_throw"/>
        <sequenceFlow id="f2" sourceRef="msg_throw" targetRef="end"/>
      </process>
    </definitions>"#;
    let def = parse_bpmn(bpmn).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(HashMap::new()).unwrap();

    assert!(result.is_completed);
    let summary = visit_summary(&result);
    assert!(summary.contains(&("msg_throw", "MessageIntermediateThrowEvent", "Activated")));
    assert!(summary.contains(&(
        "msg_throw",
        "MessageIntermediateThrowEvent",
        "MessageThrown"
    )));
}

#[test]
fn visited_escalation_records_escalation_thrown() {
    let bpmn = r#"<?xml version="1.0"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
      <escalation id="esc1" escalationCode="REVIEW_NEEDED"/>
      <process id="p" isExecutable="true">
        <startEvent id="start"><outgoing>f1</outgoing></startEvent>
        <intermediateThrowEvent id="esc_throw">
          <outgoing>f2</outgoing>
          <escalationEventDefinition escalationRef="esc1"/>
        </intermediateThrowEvent>
        <endEvent id="end"><incoming>f2</incoming></endEvent>
        <sequenceFlow id="f1" sourceRef="start" targetRef="esc_throw"/>
        <sequenceFlow id="f2" sourceRef="esc_throw" targetRef="end"/>
      </process>
    </definitions>"#;
    let def = parse_bpmn(bpmn).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(HashMap::new()).unwrap();

    assert!(result.is_completed);
    let summary = visit_summary(&result);
    assert!(summary.contains(&("esc_throw", "EscalationIntermediateThrowEvent", "Activated")));
    assert!(summary.contains(&(
        "esc_throw",
        "EscalationIntermediateThrowEvent",
        "EscalationThrown"
    )));
}

#[test]
fn visited_each_engine_call_produces_independent_visits() {
    let bpmn = r#"<?xml version="1.0"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
      <process id="p" isExecutable="true">
        <startEvent id="start"><outgoing>f1</outgoing></startEvent>
        <serviceTask id="t1"><outgoing>f2</outgoing></serviceTask>
        <serviceTask id="t2"><outgoing>f3</outgoing></serviceTask>
        <endEvent id="end"><incoming>f3</incoming></endEvent>
        <sequenceFlow id="f1" sourceRef="start" targetRef="t1"/>
        <sequenceFlow id="f2" sourceRef="t1" targetRef="t2"/>
        <sequenceFlow id="f3" sourceRef="t2" targetRef="end"/>
      </process>
    </definitions>"#;
    let def = parse_bpmn(bpmn).unwrap();
    let mut engine = Engine::new(def);

    let r1 = engine.start(HashMap::new()).unwrap();
    // start() should have: start Activated+Completed, t1 Activated
    assert!(r1.visited.iter().any(|v| v.element_id == "start"));
    assert!(r1.visited.iter().any(|v| v.element_id == "t1"));

    let r2 = engine.complete_task("t1", HashMap::new()).unwrap();
    // complete_task() should NOT contain start events — only t1 Completed + t2 Activated
    assert!(
        !r2.visited.iter().any(|v| v.element_id == "start"),
        "start should not appear in second call"
    );
    assert!(r2
        .visited
        .iter()
        .any(|v| v.element_id == "t1" && matches!(v.event, VisitEvent::Completed)));
    assert!(r2
        .visited
        .iter()
        .any(|v| v.element_id == "t2" && matches!(v.event, VisitEvent::Activated)));
}

#[test]
fn visited_element_name_is_captured() {
    let bpmn = r#"<?xml version="1.0"?>
    <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
      <process id="p" isExecutable="true">
        <startEvent id="start" name="Begin"><outgoing>f1</outgoing></startEvent>
        <scriptTask id="task" name="Process Order" scriptFormat="rhai">
          <incoming>f1</incoming><outgoing>f2</outgoing>
          <script>1</script>
        </scriptTask>
        <endEvent id="end"><incoming>f2</incoming></endEvent>
        <sequenceFlow id="f1" sourceRef="start" targetRef="task"/>
        <sequenceFlow id="f2" sourceRef="task" targetRef="end"/>
      </process>
    </definitions>"#;
    let def = parse_bpmn(bpmn).unwrap();
    let mut engine = Engine::new(def);
    let result = engine.start(HashMap::new()).unwrap();

    assert!(result.is_completed);
    let start_visit = result
        .visited
        .iter()
        .find(|v| v.element_id == "start")
        .unwrap();
    assert_eq!(start_visit.element_name.as_deref(), Some("Begin"));

    let task_visit = result
        .visited
        .iter()
        .find(|v| v.element_id == "task")
        .unwrap();
    assert_eq!(task_visit.element_name.as_deref(), Some("Process Order"));

    let end_visit = result
        .visited
        .iter()
        .find(|v| v.element_id == "end")
        .unwrap();
    assert_eq!(end_visit.element_name, None);
}
