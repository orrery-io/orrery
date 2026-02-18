use crate::components::status_badge::relative_time;
use leptos::*;
use orrery_types::TimerResponse;

#[component]
pub fn JobsTab(
    timers_data: ReadSignal<Option<Vec<TimerResponse>>>,
    acting: ReadSignal<bool>,
    on_fast_forward: Callback<String>,
    on_update: Callback<(String, String)>,
) -> impl IntoView {
    // Anti-flicker guard: freeze the local timer list while any row is being edited.
    // Without this, every poll cycle re-creates PendingTimerRow components and resets
    // their local editing state, discarding any in-progress edit.
    let (editing_timer_id, set_editing_timer_id) = create_signal(Option::<String>::None);
    let (local_timers, set_local_timers) = create_signal(Option::<Vec<TimerResponse>>::None);

    create_effect(move |_| {
        if let Some(data) = timers_data.get() {
            if editing_timer_id.get_untracked().is_none() {
                set_local_timers.set(Some(data));
            }
        }
    });

    let on_edit_start = Callback::new(move |id: String| set_editing_timer_id.set(Some(id)));
    let on_edit_end = Callback::new(move |_: ()| set_editing_timer_id.set(None));

    view! {
        {move || match local_timers.get() {
            None => view! {
                <p class="p-4 text-xs text-gray-400">"Loading…"</p>
            }.into_view(),
            Some(timers) if timers.is_empty() => view! {
                <p class="p-4 text-xs text-gray-400 italic">"No scheduled timers for this instance."</p>
            }.into_view(),
            Some(timers) => {
                let mut pending: Vec<TimerResponse> = timers.iter().filter(|t| !t.fired).cloned().collect();
                let fired: Vec<TimerResponse> = timers.iter().filter(|t| t.fired).cloned().collect();
                // Sort pending by due_at ascending (earliest first)
                pending.sort_by_key(|t| t.due_at);

                // Group fired timers by element_id so cycle repetitions show as a summary
                let mut by_element: std::collections::BTreeMap<String, Vec<TimerResponse>> = Default::default();
                for t in fired {
                    by_element.entry(t.element_id.clone()).or_default().push(t);
                }

                view! {
                    <div class="divide-y divide-gray-100 dark:divide-gray-800">
                        {pending.into_iter().map(|timer| {
                            view! { <PendingTimerRow timer acting on_fast_forward on_update on_edit_start on_edit_end /> }
                        }).collect_view()}
                        {by_element.into_iter().map(|(_, mut firings)| {
                            // Sort firings by fired_at descending (most recent first)
                            firings.sort_by(|a, b| b.fired_at.cmp(&a.fired_at));
                            let count = firings.len();
                            let latest = firings.into_iter().next().unwrap();
                            if count > 1 {
                                view! { <FiredCycleRow timer=latest count /> }
                            } else {
                                view! { <FiredTimerRow timer=latest /> }
                            }
                        }).collect_view()}
                    </div>
                }.into_view()
            }
        }}
    }
}

#[component]
fn PendingTimerRow(
    timer: TimerResponse,
    acting: ReadSignal<bool>,
    on_fast_forward: Callback<String>,
    on_update: Callback<(String, String)>,
    on_edit_start: Callback<String>,
    on_edit_end: Callback<()>,
) -> impl IntoView {
    let timer_id = timer.id.clone();
    let timer_id2 = timer.id.clone();
    let timer_id3 = timer.id.clone();
    let element_id = timer.element_id.clone();
    let expression_label = timer.expression.clone().unwrap_or_default();
    let kind_label = match timer.kind.as_str() {
        "date" => "DATE",
        "cycle" => "CYCLE",
        _ => "DURATION",
    };

    // Due-in countdown (updates every 5s)
    let due_at = timer.due_at;
    let (due_display, set_due_display) = create_signal(due_in_label(due_at));
    spawn_local(async move {
        loop {
            gloo_timers::future::sleep(std::time::Duration::from_secs(5)).await;
            set_due_display.set(due_in_label(due_at));
        }
    });

    // Inline edit state — pre-fill with current expression
    let (editing, set_editing) = create_signal(false);
    let (edit_value, set_edit_value) = create_signal(expression_label.clone());

    view! {
        <div class="flex items-start gap-3 px-4 py-3">
            // Status icon
            <span class="mt-0.5 text-amber-500 text-sm flex-shrink-0">"⏳"</span>
            // Main content
            <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2 flex-wrap">
                    <span class="font-mono text-xs text-gray-700 dark:text-gray-300">
                        {element_id}
                    </span>
                    // Kind badge
                    <span class="px-1.5 py-0.5 text-xs rounded bg-amber-100 dark:bg-amber-900/30 \
                                 text-amber-700 dark:text-amber-400 font-semibold uppercase tracking-wide">
                        {kind_label}
                    </span>
                    {(!expression_label.is_empty()).then(|| view! {
                        <span class="px-1.5 py-0.5 text-xs rounded bg-gray-100 dark:bg-gray-800 \
                                     text-gray-500 dark:text-gray-400 font-mono">
                            {expression_label}
                        </span>
                    })}
                    <span class="text-xs text-amber-600 dark:text-amber-400">
                        {move || due_display.get()}
                    </span>
                </div>
                // Inline edit form
                {move || editing.get().then(|| {
                    let timer_id_save = timer_id2.clone();
                    // Clone callbacks so each on:click handler can own its copy while the
                    // outer reactive closure retains the original for subsequent re-runs.
                    let on_edit_end_save = on_edit_end.clone();
                    let on_edit_end_cancel = on_edit_end.clone();
                    view! {
                        <div class="flex items-center gap-2 mt-2">
                            <input
                                type="text"
                                placeholder="e.g. PT10M or 2026-06-01T12:00:00Z"
                                class="text-xs px-2 py-1 rounded border border-gray-300 dark:border-gray-600 \
                                       bg-white dark:bg-gray-800 text-gray-800 dark:text-gray-200 font-mono w-56"
                                prop:value=move || edit_value.get()
                                on:input=move |ev| set_edit_value.set(event_target_value(&ev))
                            />
                            <button
                                class="px-2 py-1 text-xs rounded bg-indigo-600 text-white \
                                       hover:bg-indigo-700 cursor-pointer disabled:opacity-50"
                                disabled=move || acting.get()
                                on:click=move |_| {
                                    let val = edit_value.get();
                                    if !val.is_empty() {
                                        on_update.call((timer_id_save.clone(), val));
                                        set_editing.set(false);
                                        on_edit_end_save.call(());
                                    }
                                }
                            >"Save"</button>
                            <button
                                class="px-2 py-1 text-xs rounded border border-gray-300 dark:border-gray-600 \
                                       text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-800 \
                                       cursor-pointer"
                                on:click=move |_| {
                                    set_editing.set(false);
                                    on_edit_end_cancel.call(());
                                }
                            >"Cancel"</button>
                        </div>
                    }
                })}
            </div>
            // Action buttons
            <div class="flex items-center gap-2 flex-shrink-0">
                {move || (!editing.get()).then(|| {
                    let timer_id_ff = timer_id.clone();
                    // Clone callback and id so each on:click handler can own its copy.
                    let on_edit_start_clone = on_edit_start.clone();
                    let timer_id3_clone = timer_id3.clone();
                    view! {
                        <>
                            <button
                                class="px-2.5 py-1 text-xs rounded border border-indigo-300 dark:border-indigo-700 \
                                       text-indigo-600 dark:text-indigo-400 hover:bg-indigo-50 dark:hover:bg-indigo-950/30 \
                                       disabled:opacity-50 transition-colors cursor-pointer"
                                disabled=move || acting.get()
                                on:click=move |_| on_fast_forward.call(timer_id_ff.clone())
                            >"Fast Forward"</button>
                            <button
                                class="px-2.5 py-1 text-xs rounded border border-gray-300 dark:border-gray-600 \
                                       text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-800 \
                                       transition-colors cursor-pointer"
                                on:click=move |_| {
                                    set_editing.set(true);
                                    on_edit_start_clone.call(timer_id3_clone.clone());
                                }
                            >"Edit"</button>
                        </>
                    }
                })}
            </div>
        </div>
    }
}

#[component]
fn FiredTimerRow(timer: TimerResponse) -> impl IntoView {
    let element_id = timer.element_id.clone();
    let duration_label = timer.expression.clone().unwrap_or_default();
    let fired_label = timer
        .fired_at
        .map(|t| format!("Fired {}", relative_time(&t)))
        .unwrap_or_else(|| "Fired".to_string());

    view! {
        <div class="flex items-start gap-3 px-4 py-3 opacity-50">
            <span class="mt-0.5 text-emerald-500 text-sm flex-shrink-0">"✓"</span>
            <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2 flex-wrap">
                    <span class="font-mono text-xs text-gray-600 dark:text-gray-400">
                        {element_id}
                    </span>
                    {(!duration_label.is_empty()).then(|| view! {
                        <span class="px-1.5 py-0.5 text-xs rounded bg-gray-100 dark:bg-gray-800 \
                                     text-gray-400 dark:text-gray-500 font-mono">
                            {duration_label}
                        </span>
                    })}
                    <span class="text-xs text-gray-400">{fired_label}</span>
                </div>
            </div>
        </div>
    }
}

/// Compact row shown when a cycle timer has fired multiple times.
/// Shows a summary "Fired N times, last X ago" without listing each firing.
#[component]
fn FiredCycleRow(timer: TimerResponse, count: usize) -> impl IntoView {
    let element_id = timer.element_id.clone();
    let expression_label = timer.expression.clone().unwrap_or_default();
    let last_label = timer
        .fired_at
        .map(|t| format!("last {}", relative_time(&t)))
        .unwrap_or_else(|| "last unknown".to_string());

    view! {
        <div class="flex items-start gap-3 px-4 py-3 opacity-50">
            <span class="mt-0.5 text-emerald-500 text-sm flex-shrink-0">"✓"</span>
            <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2 flex-wrap">
                    <span class="font-mono text-xs text-gray-600 dark:text-gray-400">
                        {element_id}
                    </span>
                    <span class="px-1.5 py-0.5 text-xs rounded bg-gray-100 dark:bg-gray-800 \
                                 text-gray-400 dark:text-gray-500 font-semibold uppercase tracking-wide">
                        "CYCLE"
                    </span>
                    {(!expression_label.is_empty()).then(|| view! {
                        <span class="px-1.5 py-0.5 text-xs rounded bg-gray-100 dark:bg-gray-800 \
                                     text-gray-400 dark:text-gray-500 font-mono">
                            {expression_label}
                        </span>
                    })}
                    <span class="text-xs text-gray-400">
                        {format!("Fired {count} times, {last_label}")}
                    </span>
                </div>
            </div>
        </div>
    }
}

/// Returns a human-readable "due in X" or "overdue by X" label for a future timestamp.
fn due_in_label(due_at: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let diff = due_at - now;
    let secs = diff.num_seconds();
    if secs >= 0 {
        if secs < 60 {
            format!("due in {}s", secs)
        } else if secs < 3600 {
            format!("due in {}m {}s", secs / 60, secs % 60)
        } else {
            format!("due in {}h {}m", secs / 3600, (secs % 3600) / 60)
        }
    } else {
        let past = -secs;
        if past < 60 {
            format!("{}s overdue", past)
        } else {
            format!("{}m overdue", past / 60)
        }
    }
}
