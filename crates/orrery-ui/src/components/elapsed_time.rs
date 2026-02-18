use chrono::{DateTime, Utc};
use leptos::*;

fn format_duration(secs: i64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// Shows elapsed time for running instances (live-updating) or total time for completed.
#[component]
pub fn ElapsedTime(created_at: DateTime<Utc>, ended_at: Option<DateTime<Utc>>) -> impl IntoView {
    if let Some(end) = ended_at {
        let secs = (end - created_at).num_seconds().max(0);
        return view! {
            <span class="text-xs text-gray-500 dark:text-gray-400">
                "took " {format_duration(secs)}
            </span>
        }
        .into_view();
    }

    let now_secs = move || (Utc::now() - created_at).num_seconds().max(0);
    let (elapsed, set_elapsed) = create_signal(now_secs());

    spawn_local(async move {
        loop {
            gloo_timers::future::sleep(std::time::Duration::from_secs(1)).await;
            set_elapsed.set(now_secs());
        }
    });

    view! {
        <span class="text-xs text-gray-500 dark:text-gray-400">
            "running for " {move || format_duration(elapsed.get())}
        </span>
    }
    .into_view()
}
