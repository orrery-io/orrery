use leptos::*;

/// Returns Tailwind classes for a status dot.
pub fn state_classes(state: &str) -> &'static str {
    match state {
        "RUNNING" => "bg-blue-500",
        "WAITING_FOR_TASK" => "bg-amber-500",
        "WAITING_FOR_TIMER" => "bg-amber-500",
        "WAITING_FOR_MESSAGE" => "bg-amber-500",
        "WAITING_FOR_SIGNAL" => "bg-amber-500",
        "COMPLETED" => "bg-emerald-500",
        "FAILED" => "bg-red-500",
        "CLAIMED" => "bg-violet-500",
        "CANCELLED" => "bg-gray-500",
        _ => "bg-gray-400",
    }
}

/// Returns a human-readable label for a state.
pub fn state_label(state: &str) -> &'static str {
    match state {
        "RUNNING" => "Running",
        "WAITING_FOR_TASK" => "Waiting",
        "WAITING_FOR_TIMER" => "Timer",
        "WAITING_FOR_MESSAGE" => "Message",
        "WAITING_FOR_SIGNAL" => "Signal",
        "COMPLETED" => "Completed",
        "FAILED" => "Failed",
        "CLAIMED" => "Claimed",
        "CANCELLED" => "Cancelled",
        _ => "Unknown",
    }
}

#[component]
pub fn StatusBadge(state: String) -> impl IntoView {
    let dot = state_classes(&state);
    let label = state_label(&state);
    view! {
        <span class="inline-flex items-center gap-1.5">
            <span class=format!("inline-block w-2 h-2 rounded-full {dot}")></span>
            <span class="text-xs font-medium text-gray-700 dark:text-gray-300">{label}</span>
        </span>
    }
}

/// Truncate a UUID/ID to first 8 chars + "…"
pub fn truncate_id(id: &str) -> String {
    if id.len() > 8 {
        format!("{}…", &id[..8])
    } else {
        id.to_string()
    }
}

/// Component that shows a relative timestamp and re-computes every 30 seconds.
#[component]
pub fn RelativeTime(ts: chrono::DateTime<chrono::Utc>) -> impl IntoView {
    let (display, set_display) = create_signal(relative_time(&ts));
    spawn_local(async move {
        loop {
            gloo_timers::future::sleep(std::time::Duration::from_secs(30)).await;
            set_display.set(relative_time(&ts));
        }
    });
    view! { <span>{move || display.get()}</span> }
}

/// Relative time from a UTC DateTime.
pub fn relative_time(ts: &chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let secs = (now - *ts).num_seconds();
    if secs < 60 {
        format!("{}s ago", secs)
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}
