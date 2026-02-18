use leptos::*;
use orrery_types::{ProcessInstanceResponse, TaskResponse};

#[component]
pub fn TaskBanner(
    instance_data: ReadSignal<Option<ProcessInstanceResponse>>,
    tasks_data: ReadSignal<Option<Vec<TaskResponse>>>,
    acting: ReadSignal<bool>,
    on_retry: Callback<String>,
    open_error: Callback<()>,
) -> impl IntoView {
    move || {
        let inst = instance_data.get();
        let task_list = tasks_data.get();
        match (inst, task_list) {
            (Some(inst), Some(tasks))
                if matches!(inst.state.as_str(), "WAITING_FOR_TASK" | "FAILED") =>
            {
                let active: Vec<_> = tasks
                    .into_iter()
                    .filter(|t| matches!(t.state.as_str(), "CREATED" | "FAILED"))
                    .collect();
                if active.is_empty() {
                    return None;
                }
                Some(view! {
                    <div class="mb-4 flex flex-col gap-2">
                        {active.into_iter().map(|task| {
                            let is_failed_task = task.state == "FAILED";
                            let tid = task.id.clone();
                            view! {
                                <div class=move || {
                                    if is_failed_task {
                                        "flex items-center gap-3 px-4 py-2.5 rounded-lg border \
                                         border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-950/30"
                                    } else {
                                        "flex items-center gap-3 px-4 py-2.5 rounded-lg border \
                                         border-amber-200 dark:border-amber-800 bg-amber-50 dark:bg-amber-950/30"
                                    }
                                }>
                                    <span class=move || {
                                        if is_failed_task {
                                            "text-red-600 dark:text-red-400 text-xs font-medium uppercase tracking-wider shrink-0"
                                        } else {
                                            "text-amber-600 dark:text-amber-400 text-xs font-medium uppercase tracking-wider shrink-0"
                                        }
                                    }>
                                        {if is_failed_task { "Failed at" } else { "Waiting for" }}
                                    </span>
                                    <code class="text-xs font-mono text-gray-700 dark:text-gray-300 flex-1">
                                        {task.element_id}
                                    </code>
                                    {(task.max_retries > 0).then(|| view! {
                                        <span class=move || {
                                            if is_failed_task {
                                                "text-xs text-red-600 dark:text-red-400 shrink-0"
                                            } else {
                                                "text-xs text-amber-600 dark:text-amber-400 shrink-0"
                                            }
                                        }>
                                            "Attempt " {task.retry_count + 1} " of " {task.max_retries + 1}
                                        </span>
                                    })}
                                    {is_failed_task.then(|| {
                                        view! {
                                            <button
                                                disabled=move || acting.get()
                                                on:click=move |_| on_retry.call(tid.clone())
                                                class="px-2 py-0.5 text-xs rounded border border-amber-400 \
                                                       text-amber-700 dark:text-amber-400 hover:bg-amber-50 \
                                                       dark:hover:bg-amber-950/30 disabled:opacity-50 shrink-0 cursor-pointer"
                                            >"Retry"</button>
                                            <button
                                                on:click=move |_| open_error.call(())
                                                class="px-2 py-0.5 text-xs rounded border border-red-300 \
                                                       text-red-600 dark:text-red-400 hover:bg-red-50 \
                                                       dark:hover:bg-red-950/30 shrink-0 cursor-pointer"
                                            >"Error"</button>
                                        }
                                    })}
                                </div>
                            }
                        }).collect_view()}
                    </div>
                })
            }
            _ => None,
        }
    }
}
