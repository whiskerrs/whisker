mod api;
mod design;
mod hooks;
mod state;
mod storage;
mod ui;

#[whisker::main]
pub fn app() -> whisker::Element {
    ui::root()
}
