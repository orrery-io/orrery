use crate::components::status_badge::relative_time;
use leptos::*;
use orrery_types::HistoryEntryResponse;

#[component]
pub fn ActivityTab(
    history_data: ReadSignal<Option<Vec<HistoryEntryResponse>>>,
    level: ReadSignal<String>,
    set_level: WriteSignal<String>,
) -> impl IntoView {
    let (expanded, set_expanded) = create_signal(std::collections::HashSet::<i64>::new());

    view! {
        // Toggle bar
        <div class="flex items-center justify-end px-4 py-2 border-b border-gray-100 dark:border-gray-900">
            <button
                class="text-xs px-2 py-1 rounded border border-gray-200 dark:border-gray-700 \
                       text-gray-500 hover:bg-gray-50 dark:hover:bg-gray-800 transition-colors cursor-pointer"
                on:click=move |_| {
                    let new_level = if level.get() == "activity" { "full" } else { "activity" };
                    set_level.set(new_level.to_string());
                }
            >
                {move || if level.get() == "activity" { "Show Full Flow" } else { "Show Activity Only" }}
            </button>
        </div>
        {move || match history_data.get() {
            None => view! {
                <p class="p-4 text-xs text-gray-400">"Loading\u{2026}"</p>
            }.into_view(),
            Some(entries) if entries.is_empty() => view! {
                <p class="p-4 text-xs text-gray-400 italic">"No history yet."</p>
            }.into_view(),
            Some(entries) => {
                let expanded = expanded.clone();
                let set_expanded = set_expanded.clone();
                view! {
                    <table class="w-full text-xs">
                        <thead>
                            <tr class="border-b border-gray-100 dark:border-gray-900">
                                <th class="text-left py-1.5 px-4 font-medium text-gray-500">"Age"</th>
                                <th class="text-left py-1.5 px-4 font-medium text-gray-500">"Element"</th>
                                <th class="text-left py-1.5 px-4 font-medium text-gray-500">"Type"</th>
                                <th class="text-left py-1.5 px-4 font-medium text-gray-500">"Event"</th>
                            </tr>
                        </thead>
                        <tbody>
                            {entries.into_iter().map(|entry| {
                                let age = relative_time(&entry.occurred_at);
                                let full_ts = entry.occurred_at.to_rfc3339();
                                let is_failed = entry.event_type == "ELEMENT_FAILED"
                                    || entry.event_type == "ERROR_THROWN"
                                    || entry.event_type == "TERMINATED";
                                let entry_id = entry.id;
                                let icon = element_type_icon(&entry.element_type);
                                let el_type = entry.element_type.clone();
                                let el_name = entry.element_name.clone();
                                let el_id = entry.element_id.clone();
                                let vars = entry.variables_snapshot.clone();

                                let expanded = expanded.clone();
                                let set_expanded = set_expanded.clone();

                                view! {
                                    <tr
                                        class=move || {
                                            let base = "border-b border-gray-100 dark:border-gray-900 cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-800/50";
                                            if is_failed {
                                                format!("{base} border-l-2 border-l-red-400")
                                            } else {
                                                base.to_string()
                                            }
                                        }
                                        on:click={
                                            let set_expanded = set_expanded.clone();
                                            move |_| {
                                                set_expanded.update(|s| {
                                                    if !s.remove(&entry_id) {
                                                        s.insert(entry_id);
                                                    }
                                                });
                                            }
                                        }
                                    >
                                        <td class="py-1.5 px-4 text-gray-400 whitespace-nowrap" title=full_ts>{age}</td>
                                        <td class="py-1.5 px-4">
                                            {el_name.as_ref().map(|name| view! {
                                                <span class="text-gray-700 dark:text-gray-300">{name.clone()}</span>
                                                <br/>
                                            })}
                                            <span class="font-mono text-gray-400 dark:text-gray-500 text-[11px]">{el_id}</span>
                                        </td>
                                        <td class="py-1.5 px-4 whitespace-nowrap text-gray-500 dark:text-gray-400">
                                            <span class="mr-1">{icon}</span>
                                            {el_type}
                                        </td>
                                        <td class="py-1.5 px-4">
                                            <EventTypeChip event_type=entry.event_type.clone()/>
                                        </td>
                                    </tr>
                                    {move || expanded.get().contains(&entry_id).then(|| view! {
                                        <tr class="bg-gray-50 dark:bg-gray-900/50">
                                            <td colspan="4" class="px-4 py-2">
                                                <pre class="text-xs font-mono text-gray-600 dark:text-gray-400 whitespace-pre-wrap max-h-48 overflow-auto">
                                                    {serde_json::to_string_pretty(&vars).unwrap_or_default()}
                                                </pre>
                                            </td>
                                        </tr>
                                    })}
                                }
                            }).collect_view()}
                        </tbody>
                    </table>
                }.into_view()
            },
        }}
    }
}

fn element_type_icon(element_type: &str) -> &'static str {
    match element_type {
        t if t.contains("Timer") => "\u{23F1}",
        t if t.contains("Message") || t == "ReceiveTask" => "\u{2709}",
        t if t.contains("Signal") => "\u{1F4E1}",
        t if t.contains("Service") || t == "MultiInstanceTask" => "\u{2699}",
        t if t.contains("Script") => "\u{1F4DC}",
        t if t.contains("Gateway") => "\u{25C7}",
        t if t.contains("SubProcess") => "\u{1F4E6}",
        t if t.contains("Start") => "\u{25B6}",
        t if t.contains("End") || t.contains("Terminate") => "\u{23F9}",
        t if t.contains("Escalation") => "\u{2B06}",
        t if t.contains("Error") => "\u{26A0}",
        t if t.contains("Link") => "\u{1F517}",
        t if t.contains("Boundary") => "\u{1F6A7}",
        _ => "\u{2022}",
    }
}

#[component]
fn EventTypeChip(event_type: String) -> impl IntoView {
    let (class, label) = match event_type.as_str() {
        "ELEMENT_ACTIVATED" => ("px-1.5 py-0.5 text-xs rounded bg-blue-100 dark:bg-blue-950/50 text-blue-800 dark:text-blue-300", "Activated"),
        "ELEMENT_COMPLETED" => ("px-1.5 py-0.5 text-xs rounded bg-emerald-100 dark:bg-emerald-950/50 text-emerald-800 dark:text-emerald-300", "Completed"),
        "ELEMENT_FAILED"    => ("px-1.5 py-0.5 text-xs rounded bg-red-100 dark:bg-red-950/50 text-red-800 dark:text-red-300", "Failed"),
        "ERROR_THROWN"      => ("px-1.5 py-0.5 text-xs rounded bg-red-100 dark:bg-red-950/50 text-red-800 dark:text-red-300", "Error Thrown"),
        "ESCALATION_THROWN" => ("px-1.5 py-0.5 text-xs rounded bg-orange-100 dark:bg-orange-950/50 text-orange-800 dark:text-orange-300", "Escalation"),
        "MESSAGE_THROWN"    => ("px-1.5 py-0.5 text-xs rounded bg-sky-100 dark:bg-sky-950/50 text-sky-800 dark:text-sky-300", "Message Thrown"),
        "LINK_JUMPED"       => ("px-1.5 py-0.5 text-xs rounded bg-purple-100 dark:bg-purple-950/50 text-purple-800 dark:text-purple-300", "Link Jump"),
        "TERMINATED"        => ("px-1.5 py-0.5 text-xs rounded bg-red-200 dark:bg-red-950/70 text-red-900 dark:text-red-200 font-semibold", "Terminated"),
        _                   => ("px-1.5 py-0.5 text-xs rounded bg-gray-100 dark:bg-gray-800 text-gray-700 dark:text-gray-300", event_type.as_str()),
    };
    view! { <span class=class>{label.to_string()}</span> }
}
