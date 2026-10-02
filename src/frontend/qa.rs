//! Opt-in real-WebView test harness. Excluded from ordinary desktop builds.
use super::Dispatch;
use dioxus::prelude::*;
pub fn use_qa(d: Dispatch) {
    use_future(move || async move {
        let Some(report_path) = std::env::var_os("GOSH_MIDI_UI_REPORT") else {
            return;
        };
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let mut evaluation = document::eval(include_str!("../../tests/desktop_qa.js"));
        while let Ok(report) = evaluation.recv::<serde_json::Value>().await {
            if let Err(error) = crate::platform::persistence::write_atomic(
                std::path::Path::new(&report_path),
                serde_json::to_vec_pretty(&report).unwrap_or_default(),
            ) {
                tracing::error!(%error,"Could not write UI test report");
            }
            if report["complete"].as_bool() == Some(true) {
                break;
            }
        }
        let _ = d;
    });
}
