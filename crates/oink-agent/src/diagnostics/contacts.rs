pub(crate) fn route_event(record: oink_routing::router::TouchpadRoute, recorder: &super::Recorder) -> Event {
    let keys = [
        "input",
        "kind",
        "phase",
        "path",
        "reason",
        "allowed",
        "epoch",
        "touchpad_options",
        "max_contacts",
        "accept_contacts",
        "touchpad_injection",
        "native_multifinger",
        "mac_scroll_feel",
    ];
    numeric_event(3, Some(record.source), &keys, &record.fields, recorder)
}
pub(crate) fn numeric_event(
    stage: u8,
    source: Option<oink_foundation::DeviceId>,
    keys: &[&str],
    values: &[u64],
    recorder: &super::Recorder,
) -> Event {
    Event::Log {
        module: "oink_touchpad_diagnostics".into(),
        line: 0,
        level: 2,
        fields: std::iter::once(("stage".into(), f64::from(stage)))
            .chain(keys.iter().zip(values).map(|(k, v)| ((*k).into(), *v as f64)))
            .collect(),
        labels: source
            .into_iter()
            .map(|source| ("source".into(), recorder.anonymizer().device(source)))
            .collect(),
    }
}

/// Injection completion is an API result, not proof that an application acted.
pub(crate) fn injection_result(
    event: &oink_input::InjectEvent,
    result: &Result<(), oink_input::InputError>,
) -> Option<[u64; 5]> {
    use oink_foundation::input::{Gesture, InjectEvent};
    let (input, kind, phase, boundary) = match event {
        InjectEvent::Gesture(g) => {
            let (kind, phase) = match *g {
                Gesture::DockSwipe { axis, phase, .. } => (axis as u8, phase),
                Gesture::Magnify { phase, .. } => (3, phase),
                Gesture::Rotate { phase, .. } => (4, phase),
                Gesture::SmartMagnify => (5, oink_foundation::input::GesturePhase::Ended),
                Gesture::NotificationCenter { phase, .. } => (6, phase),
            };
            (2_u8, kind, phase as u8, g.starts() || g.ends())
        }
        InjectEvent::TouchpadContacts(f) => (1, 0, f.phase as u8, f.phase != oink_foundation::contacts::Phase::Move),
        InjectEvent::NotificationCenter => (3, 1, oink_foundation::input::GesturePhase::Ended as u8, true),
        _ => return None,
    };
    if !boundary && result.is_ok() {
        return None;
    }
    let error_kind = match result {
        Ok(()) => 0_u8,
        Err(oink_input::InputError::PermissionDenied(_)) => 1,
        Err(oink_input::InputError::Unsupported) => 2,
        Err(oink_input::InputError::NotStarted) => 3,
        Err(oink_input::InputError::Platform(_)) => 4,
    };
    Some([
        input.into(),
        kind.into(),
        phase.into(),
        u64::from(result.is_ok()),
        error_kind.into(),
    ])
}

use super::Event;
use oink_input::contact_trace::Observation;

pub(crate) fn event(o: Observation) -> Event {
    let mut fields = Vec::with_capacity(26);
    fields.extend([
        ("stage".into(), f64::from(o.stage)),
        ("device".into(), f64::from(o.device)),
        ("time_us".into(), o.time_us as f64),
        ("tick".into(), f64::from(o.tick)),
        ("count".into(), f64::from(o.count)),
        ("points".into(), f64::from(o.points)),
        ("ptp_active_mask".into(), f64::from(o.active_mask)),
        ("ptp_count".into(), f64::from(o.active_mask.count_ones())),
        ("ptp_known".into(), u8::from(o.known).into()),
        ("success".into(), u8::from(o.success).into()),
    ]);
    for (index, ((id, flags), slot)) in o.ids.into_iter().zip(o.flags).zip(o.slots).enumerate() {
        fields.push((format!("id{index}"), f64::from(id)));
        fields.push((format!("flags{index}"), f64::from(flags)));
        fields.push((format!("slot{}", index + 1), f64::from(slot)));
    }
    Event::Log {
        module: "touchpad_contacts".into(),
        line: 0,
        level: 2,
        fields,
        labels: vec![],
    }
}

/// Formatting happens on the ordinary recorder thread, never on the Quartz callback.
pub(crate) fn gesture_tap(o: oink_input::native_lifecycle::GestureTapObservation) -> Event {
    Event::Log {
        module: "macos_gesture_tap".into(),
        line: 0,
        level: 2,
        fields: [
            ("event_type", o.event_type as f64),
            ("event_type_field_55", o.event_type_field as f64),
            ("hid_type_110", o.hid_type as f64),
            ("phase_132", o.phase as f64),
            ("swipe_phase_134", o.swipe_phase as f64),
            ("motion_123", o.motion as f64),
            ("progress_124", o.progress),
            ("candidate_field_129", o.raw_field_129),
            ("candidate_field_130", o.raw_field_130),
            ("magnification_113", o.magnification),
            ("rotation_114", o.rotation),
            ("preflight_111", o.preflight as f64),
            ("series_type_117", o.series_type as f64),
            ("flavor_138", o.flavor as f64),
            ("gesture_progress_142", o.gesture_progress),
            ("gesture_stage_143", o.gesture_stage as f64),
            ("gesture_behavior_144", o.gesture_behavior as f64),
            ("inverted_from_device_136", o.inverted_from_device as f64),
            ("parsed_kind", f64::from(o.parsed_kind)),
            ("physical", u8::from(o.physical) as f64),
            ("own", u8::from(o.own) as f64),
            ("remote", u8::from(o.remote) as f64),
            ("tap_return_pass", u8::from(o.passed) as f64),
            ("native_timestamp_us", o.native_timestamp_us as f64),
            ("suppressed", o.suppressed as f64),
        ]
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect(),
        labels: vec![],
    }
}

/// Formatting happens on the ordinary recorder thread, never on MT's callback.
pub(crate) fn mt_event(o: oink_input::mt_trace::Observation) -> Event {
    let mut fields: Vec<(String, f64)> = [
        ("schema", 2.),
        ("kind", o.kind as f64),
        ("device", o.device as f64),
        ("callback_device", o.callback_device as f64),
        ("devices", o.devices as f64),
        ("built_in", o.built_in as f64),
        ("frame_count", o.frame_count as f64),
        ("source_us", o.source_us as f64),
        ("received_us", o.received_us as f64),
        ("source_gap_us", o.source_gap_us as f64),
        ("received_gap_us", o.received_gap_us as f64),
        ("source_gap_max_us", o.source_gap_max_us as f64),
        ("received_gap_max_us", o.received_gap_max_us as f64),
        ("source_backwards", o.source_backwards as f64),
        ("samples", o.samples as f64),
        ("suppressed", o.suppressed as f64),
        ("raw_count", o.raw_count as f64),
        ("accepted", o.accepted as f64),
        ("valid", u8::from(o.valid) as f64),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v))
    .collect();
    for (i, c) in o.contacts.iter().take(usize::from(o.raw_count).min(16)).enumerate() {
        for (k, v) in [
            ("id", c.id as f64),
            ("path_index", c.id as f64),
            ("finger_identity", c.finger_identity as f64),
            ("state", c.state as f64),
            ("frame", c.frame as f64),
            ("size_bits", c.size_bits as f64),
            ("density_bits", c.density_bits as f64),
        ] {
            fields.push((format!("{k}{i}"), v));
        }
    }
    Event::Log {
        module: "mt_contacts".into(),
        line: 0,
        level: 2,
        fields,
        labels: vec![],
    }
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;
    use crate::diagnostics::{Reason, Recorder};
    use oink_foundation::DeviceId;

    #[test]
    fn raw_gesture_tap_fields_are_exported_without_process_or_contact_identity() {
        let event = gesture_tap(oink_input::native_lifecycle::GestureTapObservation {
            event_type: 31,
            event_type_field: 31,
            hid_type: 61,
            phase: 1,
            swipe_phase: 0,
            motion: 2,
            progress: -0.25,
            raw_field_129: 0.125,
            raw_field_130: -0.25,
            magnification: 0.0,
            rotation: 0.0,
            inverted_from_device: 1,
            parsed_kind: 0,
            physical: true,
            own: false,
            remote: true,
            passed: false,
            native_timestamp_us: 1234,
            suppressed: 7,
            ..Default::default()
        });
        let Event::Log {
            module, fields, labels, ..
        } = event
        else {
            panic!("gesture tap observation must be a numeric log");
        };
        let values: std::collections::BTreeMap<String, f64> = fields.into_iter().collect();
        assert_eq!(module, "macos_gesture_tap");
        assert!(labels.is_empty());
        assert_eq!(values["event_type"], 31.);
        assert_eq!(values["hid_type_110"], 61.);
        assert_eq!(values["progress_124"], -0.25);
        assert_eq!(values["candidate_field_129"], 0.125);
        assert_eq!(values["candidate_field_130"], -0.25);
        assert_eq!(values["physical"], 1.);
        assert_eq!(values["remote"], 1.);
        assert_eq!(values["tap_return_pass"], 0.);
        assert_eq!(values["suppressed"], 7.);
        assert_eq!(values.len(), 25);
        assert!(
            !values
                .keys()
                .any(|key| key.contains("pid") || key.contains("coordinate") || key.contains("contact"))
        );

        let dir = tempfile::tempdir().unwrap();
        let recorder = Recorder::start(dir.path().into(), DeviceId([1; 32]), None, true).unwrap();
        recorder.wait_recovery();
        recorder.record_native(
            None,
            gesture_tap(oink_input::native_lifecycle::GestureTapObservation {
                event_type: 31,
                hid_type: 61,
                progress: -0.25,
                physical: true,
                remote: true,
                passed: false,
                ..Default::default()
            }),
        );
        let bundle = recorder.save(Reason::Manual).unwrap();
        let rows = super::super::tests::timeline(&bundle.path, "merged.ndjson");
        let log = rows
            .iter()
            .find_map(|row| {
                let log = row["event"].get("Log")?;
                (log["module"] == "macos_gesture_tap").then_some(log)
            })
            .unwrap();
        let archived: std::collections::BTreeMap<String, f64> =
            serde_json::from_value::<Vec<(String, f64)>>(log["fields"].clone())
                .unwrap()
                .into_iter()
                .collect();
        assert_eq!(archived["event_type"], 31.);
        assert_eq!(archived["hid_type_110"], 61.);
        assert_eq!(archived["progress_124"], -0.25);
        assert_eq!(archived["candidate_field_129"], 0.);
        assert_eq!(archived["candidate_field_130"], 0.);
        assert_eq!(archived["physical"], 1.);
        assert_eq!(archived["remote"], 1.);
        assert_eq!(archived["tap_return_pass"], 0.);
        recorder.shutdown();
    }

    #[test]
    fn raw_mt_states_frames_and_sensor_bits_survive_export_without_geometry() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = Recorder::start(dir.path().into(), DeviceId([1; 32]), None, true).unwrap();
        recorder.wait_recovery();
        let mut observation = oink_input::mt_trace::Observation {
            kind: 1,
            device: 3,
            callback_device: 42,
            devices: 2,
            built_in: 1,
            raw_count: 2,
            accepted: 1,
            valid: true,
            frame_count: 1234,
            source_us: 10000,
            received_us: 90000000,
            source_gap_us: 8000,
            source_gap_max_us: 24000,
            received_gap_max_us: 27000,
            ..Default::default()
        };
        observation.contacts[0] = oink_input::mt_trace::Contact {
            id: 7,
            finger_identity: 9,
            state: 4,
            frame: 1234,
            size_bits: 0.5f32.to_bits(),
            density_bits: f32::NAN.to_bits(),
        };
        observation.contacts[1].state = 7;
        recorder.record_native(None, mt_event(observation));
        let bundle = recorder.save(Reason::Manual).unwrap();
        let rows = super::super::tests::timeline(&bundle.path, "merged.ndjson");
        let log = rows
            .iter()
            .find_map(|r| {
                let log = r["event"].get("Log")?;
                (log["module"] == "mt_contacts").then_some(log)
            })
            .unwrap();
        let fields: std::collections::BTreeMap<String, f64> =
            serde_json::from_value::<Vec<(String, f64)>>(log["fields"].clone())
                .unwrap()
                .into_iter()
                .collect();
        assert_eq!(fields["id0"], 7.);
        assert_eq!(fields["schema"], 2.);
        assert_eq!(fields["path_index0"], 7.);
        assert_eq!(fields["finger_identity0"], 9.);
        assert_eq!(fields["state1"], 7.);
        assert_eq!(fields["frame_count"], 1234.);
        assert_eq!(fields["raw_count"], 2.);
        assert_eq!(fields["size_bits0"], f64::from(0.5f32.to_bits()));
        assert_eq!(fields["density_bits0"], f64::from(f32::NAN.to_bits()));
        assert_eq!(fields["source_gap_max_us"], 24000.);
        assert_eq!(fields["devices"], 2.);
        assert!(
            !fields
                .keys()
                .any(|k| k.contains("position") || k.contains("coordinate") || k.contains("serial"))
        );
        assert_eq!(fields.len(), 33); // Fixed metadata plus exactly two raw contacts.
        recorder.shutdown();
    }

    #[test]
    fn native_contact_slots_and_submitted_state_survive_recorder_export() {
        let dir = tempfile::tempdir().unwrap();
        let recorder = Recorder::start(dir.path().into(), DeviceId([1; 32]), None, true).unwrap();
        recorder.wait_recovery();
        for observation in [
            Observation {
                stage: 1,
                device: 1,
                time_us: 9000,
                tick: 9,
                count: 2,
                points: 3,
                ids: [1, 3, 4, 0, 0],
                flags: [0x4000, 0x4006, 0x4006, 0, 0],
                slots: [0, 0, u32::MAX, 17, 0],
                active_mask: 0b01100,
                known: true,
                success: true,
            },
            Observation {
                stage: 2,
                device: 1,
                time_us: 9000,
                known: true,
                success: true,
                ..Default::default()
            },
        ] {
            recorder.record_native(None, event(observation));
        }
        let bundle = recorder.save(Reason::Manual).unwrap();
        let rows = super::super::tests::timeline(&bundle.path, "merged.ndjson");
        let logs: Vec<_> = rows
            .iter()
            .filter_map(|r| {
                let log = r["event"].get("Log")?;
                (log["module"] == "touchpad_contacts").then_some(log)
            })
            .collect();
        assert_eq!(logs.len(), 2);
        let fields: std::collections::BTreeMap<_, _> =
            serde_json::from_value::<Vec<(String, f64)>>(logs[0]["fields"].clone())
                .unwrap()
                .into_iter()
                .collect();
        assert_eq!(fields["slot3"], f64::from(u32::MAX));
        assert_eq!(fields["slot4"], 17.);
        assert_eq!(fields["flags0"], f64::from(0x4000));
        assert_eq!(fields["id2"], 4.);
        assert_eq!(fields["ptp_active_mask"], 12.);
        assert_eq!(fields["ptp_count"], 2.);
        assert_eq!(fields["ptp_known"], 1.);
        assert_eq!(fields["points"], 3.);
        let closed: std::collections::BTreeMap<_, _> =
            serde_json::from_value::<Vec<(String, f64)>>(logs[1]["fields"].clone())
                .unwrap()
                .into_iter()
                .collect();
        assert_eq!(closed["stage"], 2.);
        assert_eq!(closed["ptp_active_mask"], 0.);
        assert_eq!(closed["count"], 0.);
        recorder.shutdown();
    }
}
