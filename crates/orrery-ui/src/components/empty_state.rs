use leptos::*;

#[component]
pub fn EmptyState(
    title: &'static str,
    #[prop(optional)] subtitle: Option<&'static str>,
) -> impl IntoView {
    view! {
        <div class="flex flex-col items-center justify-center py-12 text-center">
            <div class="w-8 h-8 rounded-full border-2 border-gray-300 dark:border-gray-600 mb-3"/>
            <p class="text-sm font-medium text-gray-700 dark:text-gray-300">{title}</p>
            {subtitle.map(|s| view! {
                <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">{s}</p>
            })}
        </div>
    }
}
