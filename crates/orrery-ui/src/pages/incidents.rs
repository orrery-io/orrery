use leptos::*;
use leptos_router::*;

use crate::api;
use crate::components::instances_table::InstancesTable;
use crate::components::status_badge::{relative_time, truncate_id};
use orrery_types::{ProcessInstanceResponse, TaskResponse};

#[component]
pub fn IncidentsPage() -> impl IntoView {
    let failed_instances = create_resource(
        || (),
        |_| async { api::list_instances(None, Some("FAILED")).await },
    );
    let failed_tasks = create_resource(
        || (),
        |_| async { api::list_tasks(Some("FAILED"), None).await },
    );

    let (inst_data, set_inst_data) = create_signal(Option::<Vec<ProcessInstanceResponse>>::None);
    let (task_data, set_task_data) = create_signal(Option::<Vec<TaskResponse>>::None);
    let (tasks_open, set_tasks_open) = create_signal(true);
    let (acting, set_acting) = create_signal(false);
    let (action_msg, set_action_msg) = create_signal(Option::<(bool, String)>::None);

    create_effect(move |_| {
        if let Some(Ok(paginated)) = failed_instances.get() {
            set_inst_data.set(Some(paginated.items));
        }
    });
    create_effect(move |_| {
        if let Some(Ok(v)) = failed_tasks.get() {
            set_task_data.set(Some(v));
        }
    });

    let inst_signal = Signal::derive(move || inst_data.get());

    // Poll every 10s
    spawn_local(async move {
        loop {
            gloo_timers::future::sleep(std::time::Duration::from_secs(10)).await;
            failed_instances.refetch();
            failed_tasks.refetch();
        }
    });

    let do_retry = move |task_id: String| {
        set_acting.set(true);
        set_action_msg.set(None);
        spawn_local(async move {
            match api::retry_task(&task_id).await {
                Ok(_) => {
                    set_action_msg.set(Some((true, "Task queued for retry.".into())));
                    failed_tasks.refetch();
                }
                Err(e) => set_action_msg.set(Some((false, e))),
            }
            set_acting.set(false);
        });
    };

    view! {
        <div class="p-6">
            <h1 class="text-xl font-semibold text-gray-900 dark:text-gray-100 mb-6">"Incidents"</h1>

            // ── Global empty state ────────────────────────────────────────────
            {move || {
                let insts = inst_data.get().unwrap_or_default();
                let tasks = task_data.get().unwrap_or_default();
                if insts.is_empty() && tasks.is_empty() {
                    return Some(view! {
                        <div class="flex items-center gap-2 px-4 py-3 rounded-lg border border-emerald-200 dark:border-emerald-800 bg-emerald-50 dark:bg-emerald-950/30 text-sm text-emerald-700 dark:text-emerald-400">
                            <span class="w-2 h-2 rounded-full bg-emerald-500 inline-block"></span>
                            "No incidents — everything is running."
                        </div>
                    });
                }
                None
            }}

            // ── Failed instances ──────────────────────────────────────────────
            {move || {
                inst_data.get().and_then(|insts| {
                    if insts.is_empty() { return None; }
                    let count = insts.len();
                    Some(view! {
                        <div class="mb-6">
                            <h2 class="text-xs font-medium uppercase tracking-wider \
                                       text-gray-500 dark:text-gray-400 mb-3">
                                "Failed Instances (" {count} ")"
                            </h2>
                            <InstancesTable
                                instances=inst_signal
                                show_definition=true
                                show_status_filter=false
                            />
                        </div>
                    })
                })
            }}

            // ── Failed tasks ──────────────────────────────────────────────────
            {move || {
                let tasks = task_data.get().unwrap_or_default();
                if tasks.is_empty() { return None; }
                Some(view! {
                    <div>
                        <button
                            class="flex items-center gap-2 text-xs font-medium uppercase tracking-wider text-gray-500 dark:text-gray-400 mb-3 cursor-pointer"
                            on:click=move |_| set_tasks_open.update(|v| *v = !*v)
                        >
                            "Failed Tasks (" {tasks.len()} ")"
                            <span class="text-gray-400">{move || if tasks_open.get() { "▲" } else { "▼" }}</span>
                        </button>

                        {move || action_msg.get().map(|(ok, msg)| view! {
                            <p class=if ok { "mb-2 text-xs text-emerald-600 dark:text-emerald-400" }
                                      else { "mb-2 text-xs text-red-500" }>{msg}</p>
                        })}

                        {tasks_open.get().then(|| view! {
                            <table class="w-full text-sm border-collapse">
                                <thead>
                                    <tr class="border-b-2 border-gray-200 dark:border-gray-800">
                                        <th class="text-left py-2 px-3 text-xs font-medium text-gray-500">"Element"</th>
                                        <th class="text-left py-2 px-3 text-xs font-medium text-gray-500">"Process"</th>
                                        <th class="text-left py-2 px-3 text-xs font-medium text-gray-500">"Instance"</th>
                                        <th class="text-left py-2 px-3 text-xs font-medium text-gray-500">"Retries"</th>
                                        <th class="text-left py-2 px-3 text-xs font-medium text-gray-500">"Age"</th>
                                        <th class="py-2 px-3 text-right text-xs font-medium text-gray-500">"Actions"</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {tasks.into_iter().map(|task| {
                                        let inst_href = format!("/instances/{}", task.process_instance_id);
                                        let def_href = format!("/definitions/{}/instances", task.process_definition_id);
                                        let short_inst = truncate_id(&task.process_instance_id);
                                        let short_def = truncate_id(&task.process_definition_id);
                                        let age = relative_time(&task.created_at);
                                        let tid = task.id.clone();
                                        view! {
                                            <tr class="border-b border-gray-100 dark:border-gray-900 border-l-2 border-l-red-400">
                                                <td class="py-2.5 px-3 font-mono text-xs text-gray-700 dark:text-gray-300">{task.element_id}</td>
                                                <td class="py-2.5 px-3">
                                                    <A href=def_href class="font-mono text-xs text-indigo-600 dark:text-indigo-400 hover:underline">{short_def}</A>
                                                </td>
                                                <td class="py-2.5 px-3">
                                                    <A href=inst_href class="font-mono text-xs text-indigo-600 dark:text-indigo-400 hover:underline">{short_inst}</A>
                                                </td>
                                                <td class="py-2.5 px-3 text-xs text-gray-500">
                                                    {task.retry_count}" / "{task.max_retries}
                                                </td>
                                                <td class="py-2.5 px-3 text-xs text-gray-500">{age}</td>
                                                <td class="py-2.5 px-3 text-right">
                                                    <button
                                                        disabled=move || acting.get()
                                                        on:click=move |_| do_retry(tid.clone())
                                                        class="px-2.5 py-1 text-xs font-medium rounded border border-amber-400 \
                                                               text-amber-700 dark:text-amber-400 hover:bg-amber-50 dark:hover:bg-amber-950/30 \
                                                               disabled:opacity-50 transition-colors cursor-pointer"
                                                    >"Retry"</button>
                                                </td>
                                            </tr>
                                        }
                                    }).collect_view()}
                                </tbody>
                            </table>
                        })}
                    </div>
                })
            }}
        </div>
    }
}
