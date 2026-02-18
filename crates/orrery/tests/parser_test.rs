use orrery::model::{FlowElement, ProcessDefinition, TimerKind};
use orrery::parser::parse_bpmn;

#[test]
fn parses_simple_workflow() {
    let xml = include_str!("fixtures/simple.bpmn");
    let def: ProcessDefinition = parse_bpmn(xml).expect("should parse");

    assert_eq!(def.id, "Process_1");
    assert_eq!(def.elements.len(), 3);
    assert_eq!(def.sequence_flows.len(), 2);
}

#[test]
fn parses_start_event() {
    let xml = include_str!("fixtures/simple.bpmn");
    let def = parse_bpmn(xml).unwrap();

    let start = def
        .elements
        .iter()
        .find(|e| matches!(e, FlowElement::StartEvent(_)))
        .unwrap();
    assert_eq!(start.id(), "StartEvent_1");
    assert_eq!(start.outgoing(), &["Flow_1"]);
}

#[test]
fn parses_service_task() {
    let xml = include_str!("fixtures/simple.bpmn");
    let def = parse_bpmn(xml).unwrap();

    let task = def
        .elements
        .iter()
        .find(|e| matches!(e, FlowElement::ServiceTask(_)))
        .unwrap();
    assert_eq!(task.id(), "ServiceTask_1");
    assert_eq!(task.outgoing(), &["Flow_2"]);
}

#[test]
fn parses_sequence_flows() {
    let xml = include_str!("fixtures/simple.bpmn");
    let def = parse_bpmn(xml).unwrap();

    let flow = def
        .sequence_flows
        .iter()
        .find(|f| f.id == "Flow_1")
        .unwrap();
    assert_eq!(flow.source_ref, "StartEvent_1");
    assert_eq!(flow.target_ref, "ServiceTask_1");
}

#[test]
fn parses_exclusive_gateway() {
    let xml = include_str!("fixtures/exclusive_gateway.bpmn");
    let def = orrery::parser::parse_bpmn(xml).expect("should parse");

    let gw = def
        .elements
        .iter()
        .find(|e| matches!(e, FlowElement::ExclusiveGateway(_)))
        .expect("should have gateway");
    assert_eq!(gw.id(), "gw1");
    assert_eq!(gw.outgoing().len(), 2);

    let cond_flow = def.sequence_flows.iter().find(|f| f.id == "sf2").unwrap();
    assert_eq!(
        cond_flow.condition_expression.as_deref(),
        Some("approved == true")
    );
}

#[test]
fn parses_timer_intermediate_event() {
    let xml = include_str!("fixtures/timer_process.bpmn");
    let def = orrery::parser::parse_bpmn(xml).expect("should parse");

    let timer = def
        .elements
        .iter()
        .find(|e| matches!(e, FlowElement::TimerIntermediateEvent(_)))
        .expect("should have timer event");
    assert_eq!(timer.id(), "timer1");
    assert_eq!(timer.outgoing(), &["sf2"]);

    if let FlowElement::TimerIntermediateEvent(t) = timer {
        let td = t.timer.as_ref().expect("should have timer definition");
        assert_eq!(td.expression, "PT1S");
        assert!(matches!(td.kind, orrery::model::TimerKind::Duration));
    }
}

#[test]
fn rejects_unknown_elements() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<bpmn:definitions xmlns:bpmn="http://www.omg.org/spec/BPMN/20100524/MODEL" id="D1">
  <bpmn:process id="P1" isExecutable="true">
    <bpmn:startEvent id="S1"><bpmn:outgoing>F1</bpmn:outgoing></bpmn:startEvent>
    <bpmn:someUnknownElement id="GW1"/>
    <bpmn:sequenceFlow id="F1" sourceRef="S1" targetRef="GW1"/>
  </bpmn:process>
</bpmn:definitions>"#;
    let result = parse_bpmn(xml);
    assert!(result.is_err(), "should reject unknown elements");
}

#[test]
fn parses_condition_with_gt_entity() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL" id="gt">
  <process id="gt-process" isExecutable="true">
    <startEvent id="start"><outgoing>sf0</outgoing></startEvent>
    <sequenceFlow id="sf0" sourceRef="start" targetRef="gw1"/>
    <exclusiveGateway id="gw1">
      <outgoing>sf1</outgoing>
      <outgoing>sf2</outgoing>
    </exclusiveGateway>
    <sequenceFlow id="sf1" sourceRef="gw1" targetRef="big_task">
      <conditionExpression>${amount} &gt; 1000</conditionExpression>
    </sequenceFlow>
    <sequenceFlow id="sf2" sourceRef="gw1" targetRef="small_task"/>
    <serviceTask id="big_task"><outgoing>sf3</outgoing></serviceTask>
    <serviceTask id="small_task"><outgoing>sf4</outgoing></serviceTask>
    <sequenceFlow id="sf3" sourceRef="big_task" targetRef="end"/>
    <sequenceFlow id="sf4" sourceRef="small_task" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;
    let def = orrery::parser::parse_bpmn(xml).expect("should parse");
    let cond_flow = def.sequence_flows.iter().find(|f| f.id == "sf1").unwrap();
    let expr = cond_flow.condition_expression.as_deref().unwrap();
    assert_eq!(
        expr, "${amount} > 1000",
        "gt entity should be unescaped; got: {:?}",
        expr
    );
}

#[test]
fn parses_timer_intermediate_event_time_date() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="t1"/>
    <intermediateCatchEvent id="t1">
      <outgoing>f2</outgoing>
      <timerEventDefinition><timeDate>2026-06-01T12:00:00Z</timeDate></timerEventDefinition>
    </intermediateCatchEvent>
    <sequenceFlow id="f2" sourceRef="t1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
    let def = parse_bpmn(xml).expect("should parse");
    let timer = def
        .elements
        .iter()
        .find(|e| matches!(e, FlowElement::TimerIntermediateEvent(_)))
        .expect("should have timer event");
    if let FlowElement::TimerIntermediateEvent(t) = timer {
        let td = t.timer.as_ref().expect("should have timer definition");
        assert_eq!(td.expression, "2026-06-01T12:00:00Z");
        assert!(matches!(td.kind, orrery::model::TimerKind::Date));
    }
}

#[test]
fn parses_timer_intermediate_event_time_cycle() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="t1"/>
    <intermediateCatchEvent id="t1">
      <outgoing>f2</outgoing>
      <timerEventDefinition><timeCycle>R3/PT10H</timeCycle></timerEventDefinition>
    </intermediateCatchEvent>
    <sequenceFlow id="f2" sourceRef="t1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
    let def = parse_bpmn(xml).expect("should parse");
    let timer = def
        .elements
        .iter()
        .find(|e| matches!(e, FlowElement::TimerIntermediateEvent(_)))
        .expect("should have timer event");
    if let FlowElement::TimerIntermediateEvent(t) = timer {
        let td = t.timer.as_ref().expect("should have timer definition");
        assert_eq!(td.expression, "R3/PT10H");
        assert!(matches!(td.kind, orrery::model::TimerKind::Cycle));
    }
}

#[test]
fn parses_external_service_task_topic() {
    let xml = std::fs::read_to_string("tests/fixtures/external_task.bpmn").unwrap();
    let def = parse_bpmn(&xml).unwrap();
    let task = def
        .elements
        .iter()
        .find(|e| e.id() == "pay")
        .expect("pay element not found");
    match task {
        orrery::model::FlowElement::ServiceTask(t) => {
            assert_eq!(t.topic.as_deref(), Some("payments"));
        }
        other => panic!("expected ServiceTask, got {other:?}"),
    }
}

#[test]
fn parses_timer_start_event_duration() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="ts1">
      <outgoing>sf1</outgoing>
      <timerEventDefinition><timeDuration>PT5M</timeDuration></timerEventDefinition>
    </startEvent>
    <sequenceFlow id="sf1" sourceRef="ts1" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
    let def = parse_bpmn(xml).expect("should parse");
    let elem = def
        .elements
        .iter()
        .find(|e| matches!(e, FlowElement::TimerStartEvent(_)))
        .expect("should have TimerStartEvent");
    assert_eq!(elem.id(), "ts1");
    if let FlowElement::TimerStartEvent(t) = elem {
        let td = t.timer.as_ref().expect("should have timer definition");
        assert_eq!(td.expression, "PT5M");
        assert!(matches!(td.kind, orrery::model::TimerKind::Duration));
    }
}

#[test]
fn parses_timer_start_event_cycle() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="ts1">
      <outgoing>sf1</outgoing>
      <timerEventDefinition><timeCycle>R3/PT10H</timeCycle></timerEventDefinition>
    </startEvent>
    <sequenceFlow id="sf1" sourceRef="ts1" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
    let def = parse_bpmn(xml).expect("should parse");
    let elem = def
        .elements
        .iter()
        .find(|e| matches!(e, FlowElement::TimerStartEvent(_)))
        .expect("should have TimerStartEvent");
    if let FlowElement::TimerStartEvent(t) = elem {
        let td = t.timer.as_ref().expect("should have timer definition");
        assert_eq!(td.expression, "R3/PT10H");
        assert!(matches!(td.kind, orrery::model::TimerKind::Cycle));
    }
}

#[test]
fn parses_timer_start_event_date() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="ts1">
      <outgoing>sf1</outgoing>
      <timerEventDefinition><timeDate>2030-01-01T00:00:00Z</timeDate></timerEventDefinition>
    </startEvent>
    <sequenceFlow id="sf1" sourceRef="ts1" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>sf2</outgoing></serviceTask>
    <sequenceFlow id="sf2" sourceRef="task1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
    let def = parse_bpmn(xml).expect("should parse");
    let elem = def
        .elements
        .iter()
        .find(|e| matches!(e, FlowElement::TimerStartEvent(_)))
        .expect("should have TimerStartEvent");
    if let FlowElement::TimerStartEvent(t) = elem {
        let td = t.timer.as_ref().expect("should have timer definition");
        assert_eq!(td.expression, "2030-01-01T00:00:00Z");
        assert!(matches!(td.kind, orrery::model::TimerKind::Date));
    }
}

#[test]
fn parses_bpmn_message_map_with_correlation_key() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:zeebe="http://camunda.org/schema/zeebe/1.0">
  <message id="Msg_1" name="money-collected">
    <extensionElements>
      <zeebe:subscription correlationKey="= orderId"/>
    </extensionElements>
  </message>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="catch1"/>
    <intermediateCatchEvent id="catch1">
      <outgoing>f2</outgoing>
      <messageEventDefinition messageRef="Msg_1"/>
    </intermediateCatchEvent>
    <sequenceFlow id="f2" sourceRef="catch1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
    let def = parse_bpmn(xml).unwrap();
    let elem = def.elements.iter().find(|e| e.id() == "catch1").unwrap();
    match elem {
        FlowElement::MessageIntermediateCatchEvent(m) => {
            assert_eq!(m.message_name, "money-collected");
            assert_eq!(m.correlation_key.as_deref(), Some("= orderId"));
        }
        other => panic!("expected MessageIntermediateCatchEvent, got {:?}", other),
    }
}

#[test]
fn parses_message_start_event() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <message id="Msg_1" name="order-placed"/>
  <process id="p">
    <startEvent id="start">
      <outgoing>f1</outgoing>
      <messageEventDefinition messageRef="Msg_1"/>
    </startEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
    let def = parse_bpmn(xml).unwrap();
    let elem = def.elements.iter().find(|e| e.id() == "start").unwrap();
    match elem {
        FlowElement::MessageStartEvent(m) => {
            assert_eq!(m.message_name, "order-placed");
        }
        other => panic!("expected MessageStartEvent, got {:?}", other),
    }
}

#[test]
fn parses_message_end_event() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <message id="Msg_1" name="order-completed"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="e"/>
    <endEvent id="e">
      <messageEventDefinition messageRef="Msg_1"/>
    </endEvent>
  </process>
</definitions>"#;
    let def = parse_bpmn(xml).unwrap();
    let elem = def.elements.iter().find(|e| e.id() == "e").unwrap();
    match elem {
        FlowElement::MessageEndEvent(m) => {
            assert_eq!(m.message_name, "order-completed");
        }
        other => panic!("expected MessageEndEvent, got {:?}", other),
    }
}

#[test]
fn parses_message_intermediate_throw_event() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <message id="Msg_1" name="notify"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="throw1"/>
    <intermediateThrowEvent id="throw1">
      <outgoing>f2</outgoing>
      <messageEventDefinition messageRef="Msg_1"/>
    </intermediateThrowEvent>
    <sequenceFlow id="f2" sourceRef="throw1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
    let def = parse_bpmn(xml).unwrap();
    let elem = def.elements.iter().find(|e| e.id() == "throw1").unwrap();
    match elem {
        FlowElement::MessageIntermediateThrowEvent(m) => {
            assert_eq!(m.message_name, "notify");
        }
        other => panic!("expected MessageIntermediateThrowEvent, got {:?}", other),
    }
}

#[test]
fn parses_message_boundary_event() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <message id="Msg_1" name="cancel-order"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <boundaryEvent id="boundary1" attachedToRef="task1" cancelActivity="true">
      <outgoing>f3</outgoing>
      <messageEventDefinition messageRef="Msg_1"/>
    </boundaryEvent>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="e"/>
    <sequenceFlow id="f3" sourceRef="boundary1" targetRef="e"/>
    <endEvent id="e"/>
  </process>
</definitions>"#;
    let def = parse_bpmn(xml).unwrap();
    let elem = def.elements.iter().find(|e| e.id() == "boundary1").unwrap();
    match elem {
        FlowElement::MessageBoundaryEvent(m) => {
            assert_eq!(m.message_name, "cancel-order");
            assert_eq!(m.attached_to_ref, "task1");
            assert!(m.is_interrupting);
        }
        other => panic!("expected MessageBoundaryEvent, got {:?}", other),
    }
}

#[test]
fn parses_camunda_namespace_external_task_topic() {
    // Verify backward-compat: Camunda BPMN files work without modification.
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:camunda="http://camunda.org/schema/1.0/bpmn"
             targetNamespace="http://example.org">
  <process id="p" isExecutable="true">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <serviceTask id="pay" camunda:type="external" camunda:topic="payments">
      <incoming>f1</incoming><outgoing>f2</outgoing>
    </serviceTask>
    <endEvent id="end"><incoming>f2</incoming></endEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="pay"/>
    <sequenceFlow id="f2" sourceRef="pay" targetRef="end"/>
  </process>
</definitions>"#;
    let def = parse_bpmn(xml).unwrap();
    let task = def.elements.iter().find(|e| e.id() == "pay").unwrap();
    match task {
        orrery::model::FlowElement::ServiceTask(t) => {
            assert_eq!(t.topic.as_deref(), Some("payments"));
        }
        other => panic!("expected ServiceTask, got {other:?}"),
    }
}

#[test]
fn parse_script_task() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:zeebe="http://camunda.org/schema/zeebe/1.0">
  <process id="Process_1" isExecutable="true">
    <startEvent id="Start_1">
      <outgoing>Flow_1</outgoing>
    </startEvent>
    <scriptTask id="Script_1" name="Calculate" scriptFormat="rhai">
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
    let script = def.elements.iter().find(|e| e.id() == "Script_1").unwrap();
    match script {
        FlowElement::ScriptTask(st) => {
            assert_eq!(st.script_format, "rhai");
            assert_eq!(st.script, "a + b");
            assert_eq!(st.result_variable.as_deref(), Some("sum"));
            assert_eq!(st.name.as_deref(), Some("Calculate"));
        }
        other => panic!("Expected ScriptTask, got {:?}", other),
    }
}

#[test]
fn parse_script_task_default_format() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="Process_1" isExecutable="true">
    <startEvent id="Start_1">
      <outgoing>Flow_1</outgoing>
    </startEvent>
    <scriptTask id="Script_1">
      <incoming>Flow_1</incoming>
      <outgoing>Flow_2</outgoing>
      <script>42</script>
    </scriptTask>
    <endEvent id="End_1">
      <incoming>Flow_2</incoming>
    </endEvent>
    <sequenceFlow id="Flow_1" sourceRef="Start_1" targetRef="Script_1" />
    <sequenceFlow id="Flow_2" sourceRef="Script_1" targetRef="End_1" />
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let script = def.elements.iter().find(|e| e.id() == "Script_1").unwrap();
    match script {
        FlowElement::ScriptTask(st) => {
            assert_eq!(st.script_format, "rhai");
            assert_eq!(st.result_variable, None);
        }
        other => panic!("Expected ScriptTask, got {:?}", other),
    }
}

#[test]
fn parse_script_task_camunda_result_variable() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"
             xmlns:camunda="http://camunda.org/schema/1.0/bpmn">
  <process id="Process_1" isExecutable="true">
    <startEvent id="Start_1">
      <outgoing>Flow_1</outgoing>
    </startEvent>
    <scriptTask id="Script_1" scriptFormat="rhai" camunda:resultVariable="total">
      <incoming>Flow_1</incoming>
      <outgoing>Flow_2</outgoing>
      <script>a * b</script>
    </scriptTask>
    <endEvent id="End_1">
      <incoming>Flow_2</incoming>
    </endEvent>
    <sequenceFlow id="Flow_1" sourceRef="Start_1" targetRef="Script_1" />
    <sequenceFlow id="Flow_2" sourceRef="Script_1" targetRef="End_1" />
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let script = def.elements.iter().find(|e| e.id() == "Script_1").unwrap();
    match script {
        FlowElement::ScriptTask(st) => {
            assert_eq!(st.result_variable.as_deref(), Some("total"));
            assert_eq!(st.script, "a * b");
        }
        other => panic!("Expected ScriptTask, got {:?}", other),
    }
}

#[test]
fn parses_timer_boundary_event_interrupting() {
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
    let elem = def
        .elements
        .iter()
        .find(|e| e.id() == "timer_bound")
        .unwrap();
    match elem {
        FlowElement::TimerBoundaryEvent(tb) => {
            assert_eq!(tb.attached_to_ref, "task1");
            assert!(tb.is_interrupting);
            assert_eq!(tb.timer.expression, "PT1H");
            assert!(matches!(tb.timer.kind, TimerKind::Duration));
            assert_eq!(tb.outgoing, vec!["f3"]);
        }
        other => panic!("expected TimerBoundaryEvent, got {:?}", other),
    }
}

#[test]
fn parses_timer_boundary_event_non_interrupting() {
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
    <sequenceFlow id="f3" sourceRef="timer_bound" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let elem = def
        .elements
        .iter()
        .find(|e| e.id() == "timer_bound")
        .unwrap();
    match elem {
        FlowElement::TimerBoundaryEvent(tb) => {
            assert!(!tb.is_interrupting);
            assert_eq!(tb.timer.expression, "PT30M");
        }
        other => panic!("expected TimerBoundaryEvent, got {:?}", other),
    }
}

#[test]
fn parses_timer_boundary_event_date() {
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
      <timerEventDefinition><timeDate>2026-12-31T23:59:00Z</timeDate></timerEventDefinition>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="timer_bound" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let elem = def
        .elements
        .iter()
        .find(|e| e.id() == "timer_bound")
        .unwrap();
    match elem {
        FlowElement::TimerBoundaryEvent(tb) => {
            assert!(matches!(tb.timer.kind, TimerKind::Date));
            assert_eq!(tb.timer.expression, "2026-12-31T23:59:00Z");
        }
        other => panic!("expected TimerBoundaryEvent, got {:?}", other),
    }
}

#[test]
fn parses_timer_boundary_event_cycle() {
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
      <timerEventDefinition><timeCycle>R3/PT10M</timeCycle></timerEventDefinition>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="timer_bound" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let elem = def
        .elements
        .iter()
        .find(|e| e.id() == "timer_bound")
        .unwrap();
    match elem {
        FlowElement::TimerBoundaryEvent(tb) => {
            assert!(!tb.is_interrupting);
            assert!(matches!(tb.timer.kind, TimerKind::Cycle));
            assert_eq!(tb.timer.expression, "R3/PT10M");
        }
        other => panic!("expected TimerBoundaryEvent, got {:?}", other),
    }
}

#[test]
fn parses_timer_boundary_default_interrupting() {
    // cancelActivity omitted — defaults to true (interrupting)
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="end1"/>
    <endEvent id="end1"/>
    <boundaryEvent id="timer_bound" attachedToRef="task1">
      <outgoing>f3</outgoing>
      <timerEventDefinition><timeDuration>PT5M</timeDuration></timerEventDefinition>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="timer_bound" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let elem = def
        .elements
        .iter()
        .find(|e| e.id() == "timer_bound")
        .unwrap();
    match elem {
        FlowElement::TimerBoundaryEvent(tb) => {
            assert!(
                tb.is_interrupting,
                "cancelActivity omitted should default to interrupting"
            );
        }
        other => panic!("expected TimerBoundaryEvent, got {:?}", other),
    }
}

#[test]
fn parses_signal_start_event() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <signal id="Sig_1" name="OrderPlaced"/>
  <process id="p">
    <startEvent id="sig_start">
      <signalEventDefinition signalRef="Sig_1"/>
      <outgoing>f1</outgoing>
    </startEvent>
    <sequenceFlow id="f1" sourceRef="sig_start" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let elem = def.elements.iter().find(|e| e.id() == "sig_start").unwrap();
    match elem {
        FlowElement::SignalStartEvent(s) => {
            assert_eq!(s.signal_ref, "OrderPlaced");
            assert_eq!(s.outgoing, vec!["f1"]);
        }
        other => panic!("expected SignalStartEvent, got {:?}", other),
    }
}

#[test]
fn parses_signal_intermediate_throw_event() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <signal id="Sig_1" name="OrderShipped"/>
  <process id="p">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <intermediateThrowEvent id="sig_throw">
      <signalEventDefinition signalRef="Sig_1"/>
      <incoming>f1</incoming>
      <outgoing>f2</outgoing>
    </intermediateThrowEvent>
    <endEvent id="end"><incoming>f2</incoming></endEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="sig_throw"/>
    <sequenceFlow id="f2" sourceRef="sig_throw" targetRef="end"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let elem = def.elements.iter().find(|e| e.id() == "sig_throw").unwrap();
    match elem {
        FlowElement::SignalIntermediateThrowEvent(s) => {
            assert_eq!(s.signal_ref, "OrderShipped");
            assert_eq!(s.outgoing, vec!["f2"]);
        }
        other => panic!("expected SignalIntermediateThrowEvent, got {:?}", other),
    }
}

#[test]
fn parses_signal_end_event() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <signal id="Sig_1" name="ProcessDone"/>
  <process id="p">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <endEvent id="sig_end">
      <signalEventDefinition signalRef="Sig_1"/>
      <incoming>f1</incoming>
    </endEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="sig_end"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let elem = def.elements.iter().find(|e| e.id() == "sig_end").unwrap();
    match elem {
        FlowElement::SignalEndEvent(s) => {
            assert_eq!(s.signal_ref, "ProcessDone");
        }
        other => panic!("expected SignalEndEvent, got {:?}", other),
    }
}

#[test]
fn parses_signal_boundary_event_interrupting() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <signal id="Sig_1" name="CancelOrder"/>
  <process id="p">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <serviceTask id="task1"><incoming>f1</incoming><outgoing>f3</outgoing></serviceTask>
    <boundaryEvent id="sig_boundary" attachedToRef="task1">
      <signalEventDefinition signalRef="Sig_1"/>
      <outgoing>f2</outgoing>
    </boundaryEvent>
    <endEvent id="end1"><incoming>f3</incoming></endEvent>
    <endEvent id="end2"><incoming>f2</incoming></endEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="task1"/>
    <sequenceFlow id="f2" sourceRef="sig_boundary" targetRef="end2"/>
    <sequenceFlow id="f3" sourceRef="task1" targetRef="end1"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let elem = def
        .elements
        .iter()
        .find(|e| e.id() == "sig_boundary")
        .unwrap();
    match elem {
        FlowElement::SignalBoundaryEvent(s) => {
            assert_eq!(s.signal_ref, "CancelOrder");
            assert_eq!(s.attached_to_ref, "task1");
            assert!(s.is_interrupting);
            assert_eq!(s.outgoing, vec!["f2"]);
        }
        other => panic!("expected SignalBoundaryEvent, got {:?}", other),
    }
}

#[test]
fn parses_signal_name_resolution_after_process() {
    // When <signal> definitions appear AFTER <process>, resolve_signal_names()
    // post-processing must resolve signalRef IDs to signal names.
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="sig_start">
      <signalEventDefinition signalRef="Sig_1"/>
      <outgoing>f1</outgoing>
    </startEvent>
    <sequenceFlow id="f1" sourceRef="sig_start" targetRef="throw1"/>
    <intermediateThrowEvent id="throw1">
      <signalEventDefinition signalRef="Sig_2"/>
      <outgoing>f2</outgoing>
    </intermediateThrowEvent>
    <sequenceFlow id="f2" sourceRef="throw1" targetRef="sig_end"/>
    <endEvent id="sig_end">
      <signalEventDefinition signalRef="Sig_3"/>
    </endEvent>
  </process>
  <signal id="Sig_1" name="StartSignal"/>
  <signal id="Sig_2" name="ThrowSignal"/>
  <signal id="Sig_3" name="EndSignal"/>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();

    // SignalStartEvent should have resolved name
    let start = def.elements.iter().find(|e| e.id() == "sig_start").unwrap();
    match start {
        FlowElement::SignalStartEvent(s) => assert_eq!(s.signal_ref, "StartSignal"),
        other => panic!("expected SignalStartEvent, got {:?}", other),
    }

    // SignalIntermediateThrowEvent should have resolved name
    let throw = def.elements.iter().find(|e| e.id() == "throw1").unwrap();
    match throw {
        FlowElement::SignalIntermediateThrowEvent(s) => assert_eq!(s.signal_ref, "ThrowSignal"),
        other => panic!("expected SignalIntermediateThrowEvent, got {:?}", other),
    }

    // SignalEndEvent should have resolved name
    let end = def.elements.iter().find(|e| e.id() == "sig_end").unwrap();
    match end {
        FlowElement::SignalEndEvent(s) => assert_eq!(s.signal_ref, "EndSignal"),
        other => panic!("expected SignalEndEvent, got {:?}", other),
    }
}

#[test]
fn parses_signal_boundary_event_non_interrupting() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <signal id="Sig_1" name="Notification"/>
  <process id="p">
    <startEvent id="start"><outgoing>f1</outgoing></startEvent>
    <serviceTask id="task1"><incoming>f1</incoming><outgoing>f3</outgoing></serviceTask>
    <boundaryEvent id="sig_boundary" attachedToRef="task1" cancelActivity="false">
      <signalEventDefinition signalRef="Sig_1"/>
      <outgoing>f2</outgoing>
    </boundaryEvent>
    <endEvent id="end1"><incoming>f3</incoming></endEvent>
    <endEvent id="end2"><incoming>f2</incoming></endEvent>
    <sequenceFlow id="f1" sourceRef="start" targetRef="task1"/>
    <sequenceFlow id="f2" sourceRef="sig_boundary" targetRef="end2"/>
    <sequenceFlow id="f3" sourceRef="task1" targetRef="end1"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let elem = def
        .elements
        .iter()
        .find(|e| e.id() == "sig_boundary")
        .unwrap();
    match elem {
        FlowElement::SignalBoundaryEvent(s) => {
            assert_eq!(s.signal_ref, "Notification");
            assert!(!s.is_interrupting);
        }
        other => panic!("expected SignalBoundaryEvent, got {:?}", other),
    }
}

#[test]
fn parses_error_end_event() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <error id="Err_1" name="PaymentFailed" errorCode="PAYMENT_ERR"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="err_end"/>
    <endEvent id="err_end">
      <errorEventDefinition errorRef="Err_1"/>
    </endEvent>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let elem = def.elements.iter().find(|e| e.id() == "err_end").unwrap();
    match elem {
        FlowElement::ErrorEndEvent(e) => {
            assert_eq!(e.error_code.as_deref(), Some("PAYMENT_ERR"));
        }
        other => panic!("expected ErrorEndEvent, got {:?}", other),
    }
}

#[test]
fn parses_error_boundary_with_error_code() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <error id="Err_1" name="PaymentFailed" errorCode="PAYMENT_ERR"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="task1"/>
    <serviceTask id="task1"><outgoing>f2</outgoing></serviceTask>
    <sequenceFlow id="f2" sourceRef="task1" targetRef="end1"/>
    <endEvent id="end1"/>
    <boundaryEvent id="err_bound" attachedToRef="task1">
      <outgoing>f3</outgoing>
      <errorEventDefinition errorRef="Err_1"/>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="err_bound" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let elem = def.elements.iter().find(|e| e.id() == "err_bound").unwrap();
    match elem {
        FlowElement::BoundaryEvent(b) => {
            assert_eq!(b.event_type, "error");
            assert_eq!(b.error_code.as_deref(), Some("PAYMENT_ERR"));
        }
        other => panic!("expected BoundaryEvent with error_code, got {:?}", other),
    }
}

#[test]
fn parses_error_end_event_without_error_code() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="err_end"/>
    <endEvent id="err_end">
      <errorEventDefinition/>
    </endEvent>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let elem = def.elements.iter().find(|e| e.id() == "err_end").unwrap();
    match elem {
        FlowElement::ErrorEndEvent(e) => {
            assert!(e.error_code.is_none());
        }
        other => panic!("expected ErrorEndEvent, got {:?}", other),
    }
}

#[test]
fn parses_terminate_end_event() {
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
    let elem = def.elements.iter().find(|e| e.id() == "term").unwrap();
    assert!(matches!(elem, FlowElement::TerminateEndEvent(_)));
}

#[test]
fn parses_terminate_end_event_non_self_closing() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="term"/>
    <endEvent id="term">
      <terminateEventDefinition></terminateEventDefinition>
    </endEvent>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let elem = def.elements.iter().find(|e| e.id() == "term").unwrap();
    assert!(matches!(elem, FlowElement::TerminateEndEvent(_)));
}

#[test]
fn parses_escalation_intermediate_throw_event() {
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
    <sequenceFlow id="f2" sourceRef="esc_throw" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let elem = def.elements.iter().find(|e| e.id() == "esc_throw").unwrap();
    match elem {
        FlowElement::EscalationIntermediateThrowEvent(e) => {
            assert_eq!(e.escalation_code.as_deref(), Some("ESC_001"));
        }
        other => panic!("expected EscalationIntermediateThrowEvent, got {:?}", other),
    }
}

#[test]
fn parses_escalation_end_event() {
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
    let elem = def.elements.iter().find(|e| e.id() == "esc_end").unwrap();
    match elem {
        FlowElement::EscalationEndEvent(e) => {
            assert_eq!(e.escalation_code.as_deref(), Some("ESC_002"));
        }
        other => panic!("expected EscalationEndEvent, got {:?}", other),
    }
}

#[test]
fn parses_escalation_boundary_event() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <escalation id="esc1" escalationCode="ESC_003"/>
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="sub"/>
    <subProcess id="sub"><outgoing>f2</outgoing>
      <startEvent id="sub_s"><outgoing>sf1</outgoing></startEvent>
      <sequenceFlow id="sf1" sourceRef="sub_s" targetRef="sub_end"/>
      <endEvent id="sub_end"/>
    </subProcess>
    <sequenceFlow id="f2" sourceRef="sub" targetRef="end"/>
    <endEvent id="end"/>
    <boundaryEvent id="esc_bound" attachedToRef="sub" cancelActivity="false">
      <outgoing>f3</outgoing>
      <escalationEventDefinition escalationRef="esc1"/>
    </boundaryEvent>
    <sequenceFlow id="f3" sourceRef="esc_bound" targetRef="end2"/>
    <endEvent id="end2"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let elem = def.elements.iter().find(|e| e.id() == "esc_bound").unwrap();
    match elem {
        FlowElement::EscalationBoundaryEvent(e) => {
            assert_eq!(e.escalation_code.as_deref(), Some("ESC_003"));
            assert_eq!(e.attached_to_ref, "sub");
            assert!(!e.is_interrupting);
        }
        other => panic!("expected EscalationBoundaryEvent, got {:?}", other),
    }
}

#[test]
fn parses_escalation_self_closing_definition() {
    let xml = r#"<?xml version="1.0"?>
<definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL">
  <process id="p">
    <startEvent id="s"><outgoing>f1</outgoing></startEvent>
    <sequenceFlow id="f1" sourceRef="s" targetRef="esc_throw"/>
    <intermediateThrowEvent id="esc_throw">
      <escalationEventDefinition/>
      <outgoing>f2</outgoing>
    </intermediateThrowEvent>
    <sequenceFlow id="f2" sourceRef="esc_throw" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();
    let elem = def.elements.iter().find(|e| e.id() == "esc_throw").unwrap();
    match elem {
        FlowElement::EscalationIntermediateThrowEvent(e) => {
            assert!(
                e.escalation_code.is_none(),
                "catch-all should have no escalation code"
            );
        }
        other => panic!("expected EscalationIntermediateThrowEvent, got {:?}", other),
    }
}

#[test]
fn parses_link_events() {
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
    <sequenceFlow id="f2" sourceRef="link_catch" targetRef="end"/>
    <endEvent id="end"/>
  </process>
</definitions>"#;

    let def = parse_bpmn(xml).unwrap();

    let throw = def
        .elements
        .iter()
        .find(|e| e.id() == "link_throw")
        .unwrap();
    match throw {
        FlowElement::LinkIntermediateThrowEvent(l) => assert_eq!(l.link_name, "jump1"),
        other => panic!("expected LinkIntermediateThrowEvent, got {:?}", other),
    }

    let catch = def
        .elements
        .iter()
        .find(|e| e.id() == "link_catch")
        .unwrap();
    match catch {
        FlowElement::LinkIntermediateCatchEvent(l) => assert_eq!(l.link_name, "jump1"),
        other => panic!("expected LinkIntermediateCatchEvent, got {:?}", other),
    }
}
