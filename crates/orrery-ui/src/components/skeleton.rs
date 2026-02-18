use leptos::*;

/// Renders `cols` animated shimmer cells in a table row.
#[component]
pub fn SkeletonRow(cols: usize) -> impl IntoView {
    view! {
        <tr>
            {(0..cols).map(|_| view! {
                <td class="py-2.5 px-3">
                    <div class="animate-pulse bg-gray-200 dark:bg-gray-800 rounded h-3 w-full"/>
                </td>
            }).collect_view()}
        </tr>
    }
}
