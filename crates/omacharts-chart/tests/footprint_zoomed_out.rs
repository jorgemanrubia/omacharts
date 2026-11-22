//! A footprint zoomed out too far for its rows costs a frame nothing: the
//! chart allocates exactly what it allocates without one, drawn on its own
//! and after a trade on the forming bar. The frame time beside it is in
//! `examples/order_flow_perf.rs`; a time is too noisy to assert here.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use gtk::cairo;
use omacharts_chart::flow_bench::FlowScene;

thread_local! {
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
}

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const WIDTH: f64 = 1600.0;
const HEIGHT: f64 = 900.0;

/// What one run of `work` allocates on this thread, once warmed.
fn allocs<T>(on: &mut T, mut work: impl FnMut(&mut T)) -> u64 {
    for _ in 0..3 {
        work(on);
    }
    let before = ALLOCS.with(Cell::get);
    work(on);
    ALLOCS.with(Cell::get) - before
}

/// The same chart, 400 bars across 1600 pixels: four a bar, far under
/// what a footprint's rows need.
fn scene(footprint: bool) -> FlowScene {
    let kinds: &[&str] = if footprint { &["volume", "footprint"] } else { &["volume"] };
    FlowScene::new(1_000, 400, kinds, 400, 0).without_legend()
}

#[test]
fn a_footprint_zoomed_out_allocates_nothing_a_chart_without_one_does_not() {
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, WIDTH as i32, HEIGHT as i32).unwrap();
    let cr = cairo::Context::new(&surface).unwrap();

    let drawn = |footprint: bool| allocs(&mut scene(footprint), |scene| scene.view_frame(&cr, WIDTH, HEIGHT));
    assert_eq!(drawn(true), drawn(false), "a frame");

    let ticked = |footprint: bool| {
        allocs(&mut scene(footprint), |scene| {
            scene.trade();
            scene.view_frame(&cr, WIDTH, HEIGHT);
        })
    };
    assert_eq!(ticked(true), ticked(false), "a trade on the forming bar, then a frame");

    // And zoomed in, the footprint is drawn: the test above compares a
    // footprint that is there, not one that never was.
    let zoomed_in = |footprint: bool| {
        let kinds: &[&str] = if footprint { &["volume", "footprint"] } else { &["volume"] };
        allocs(&mut FlowScene::new(400, 20, kinds, 400, 0).without_legend(), |scene| scene.view_frame(&cr, WIDTH, HEIGHT))
    };
    assert_ne!(zoomed_in(true), zoomed_in(false), "zoomed in, the footprint draws");
}
