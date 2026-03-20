use leptos::*;
use web_sys::MouseEvent;

#[component]
pub fn BpmnViewer(
    /// Reactive closure returning the diagram URL. Re-evaluated whenever its
    /// captured signals change, which causes the resource to refetch.
    /// Example: `move || format!("/v1/process-instances/{}/diagram?_={}", id(), state())`
    diagram_url: impl Fn() -> String + 'static,
    /// Optional callback invoked with the element id when a badge is clicked.
    #[prop(optional)]
    on_element_click: Option<Callback<String>>,
) -> impl IntoView {
    let interactive = on_element_click.is_some();
    let (scale, set_scale) = create_signal(1.0_f32);
    let (tx, set_tx) = create_signal(0.0_f32);
    let (ty, set_ty) = create_signal(0.0_f32);
    let (dragging, set_dragging) = create_signal(false);
    let (drag_x, set_drag_x) = create_signal(0.0_f32);
    let (drag_y, set_drag_y) = create_signal(0.0_f32);

    let svg = create_resource(diagram_url, |url| async move {
        match gloo_net::http::Request::get(&url).send().await {
            Ok(resp) => resp.text().await.unwrap_or_default(),
            Err(_) => String::new(),
        }
    });

    let zoom_in = move |_: MouseEvent| set_scale.update(|s| *s = (*s * 1.25).min(4.0));
    let zoom_out = move |_: MouseEvent| set_scale.update(|s| *s = (*s / 1.25).max(0.25));
    let reset = move |_: MouseEvent| {
        set_scale.set(1.0);
        set_tx.set(0.0);
        set_ty.set(0.0);
    };

    let on_mousedown = move |e: MouseEvent| {
        set_dragging.set(true);
        set_drag_x.set(e.client_x() as f32);
        set_drag_y.set(e.client_y() as f32);
        e.prevent_default();
    };

    let on_mousemove = move |e: MouseEvent| {
        if dragging.get() {
            let dx = e.client_x() as f32 - drag_x.get();
            let dy = e.client_y() as f32 - drag_y.get();
            set_tx.update(|v| *v += dx);
            set_ty.update(|v| *v += dy);
            set_drag_x.set(e.client_x() as f32);
            set_drag_y.set(e.client_y() as f32);
        }
    };

    let on_mouseup = move |_: MouseEvent| set_dragging.set(false);
    let on_mouseleave = move |_: MouseEvent| set_dragging.set(false);

    let on_dblclick = move |e: MouseEvent| {
        let old_scale = scale.get();
        let new_scale = (old_scale * 1.25).min(4.0);
        let ox = e.offset_x() as f32;
        let oy = e.offset_y() as f32;
        set_scale.set(new_scale);
        set_tx.set(ox - (ox - tx.get()) * (new_scale / old_scale));
        set_ty.set(oy - (oy - ty.get()) * (new_scale / old_scale));
    };

    view! {
        <div
            class="relative overflow-hidden border border-gray-200 dark:border-gray-800 \
                   rounded-lg bg-white dark:bg-gray-950 select-none"
            style=move || if dragging.get() { "height: 500px; cursor: grabbing" } else { "height: 500px; cursor: grab" }
            on:mousedown=on_mousedown
            on:mousemove=on_mousemove
            on:mouseup=on_mouseup
            on:mouseleave=on_mouseleave
            on:dblclick=on_dblclick
            on:click=move |ev: MouseEvent| {
                let Some(ref cb) = on_element_click else { return };
                use wasm_bindgen::JsCast;
                let target = ev.target()
                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok());
                if let Some(el) = target {
                    let found = el.closest("[data-element-id]").ok().flatten();
                    if let Some(badge_group) = found {
                        let eid = badge_group
                            .get_attribute("data-element-id")
                            .unwrap_or_default();
                        if !eid.is_empty() {
                            cb.call(eid);
                        }
                    }
                }
            }
        >
            <div class="absolute top-2 right-2 z-10 flex gap-1">
                <button
                    class="w-7 h-7 flex items-center justify-center rounded \
                           bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-300 \
                           hover:bg-gray-200 dark:hover:bg-gray-700 text-sm font-mono cursor-pointer"
                    on:click=zoom_in
                >"+"</button>
                <button
                    class="w-7 h-7 flex items-center justify-center rounded \
                           bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-300 \
                           hover:bg-gray-200 dark:hover:bg-gray-700 text-sm font-mono cursor-pointer"
                    on:click=zoom_out
                >"−"</button>
                <button
                    class="w-7 h-7 flex items-center justify-center rounded \
                           bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-300 \
                           hover:bg-gray-200 dark:hover:bg-gray-700 text-xs cursor-pointer"
                    on:click=reset
                >"⟳"</button>
            </div>

            <Suspense fallback=move || view! {
                <div class="flex items-center justify-center h-full text-sm text-gray-400">
                    "Loading diagram…"
                </div>
            }>
                {move || svg.get().map(|content| view! {
                    <div
                        style=move || format!(
                            "transform: translate({}px, {}px) scale({}); transform-origin: 0 0; \
                             width: 100%; height: 100%; pointer-events: {}",
                            tx.get(), ty.get(), scale.get(),
                            if interactive { "auto" } else { "none" }
                        )
                        inner_html=content
                    />
                })}
            </Suspense>
        </div>
    }
}
