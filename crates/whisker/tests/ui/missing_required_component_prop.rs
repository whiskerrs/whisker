use whisker::prelude::*;

#[component]
fn greeting(label: String) -> Element {
    render! { Text(value: label.clone()) }
}

fn main() {
    let _ = Greeting::builder().build();
}
