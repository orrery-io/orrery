use leptos::*;
use leptos_router::*;

/// Renders the standard page header row: `← Processes | {children}`
///
/// Wraps everything in the shared `flex items-center gap-3 mb-6` container and
/// adds the `← Processes` back-link plus a `|` separator. Each page then
/// provides its own segment content as `children`.
#[component]
pub fn PageBreadcrumb(children: Children) -> impl IntoView {
    view! {
        <div class="flex items-center gap-3 mb-6">
            <A
                href="/definitions"
                class="text-sm text-indigo-600 dark:text-indigo-400 hover:underline"
            >
                "← Processes"
            </A>
            <span class="text-gray-300 dark:text-gray-600">"|"</span>
            {children()}
        </div>
    }
}
