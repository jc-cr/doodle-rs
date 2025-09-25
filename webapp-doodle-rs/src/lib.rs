//file: lib.rs
// desc: serve webapp

use leptos::*;

#[component]
fn App() -> impl IntoView {
    let (count, set_count) = create_signal(0);

    view! {
        <div>
            <h1>"Doodle-RS"</h1>
            <p>"Canvas placeholder coming soon..."</p>
            <button
                on:click=move |_| {
                    set_count.update(|n| *n += 1);
                }
            >
                "Click me: " {count}
            </button>
        </div>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount_to_body(App)
}