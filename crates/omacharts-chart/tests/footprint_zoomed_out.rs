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

    // And zoomed in, the footprint is drawn, its rows and figures with
    // nothing allocated for them either: the test above compares a
    // footprint that is there, not one that never was.
    let zoomed_in = |footprint: bool| {
        let kinds: &[&str] = if footprint { &["volume", "footprint"] } else { &["volume"] };
        let mut scene = FlowScene::new(400, 20, kinds, 400, 0).without_legend();
        let allocated = allocs(&mut scene, |scene| scene.view_frame(&cr, WIDTH, HEIGHT));
        let mut drawn = cairo::ImageSurface::create(cairo::Format::ARgb32, WIDTH as i32, HEIGHT as i32).unwrap();
        {
            let cr = cairo::Context::new(&drawn).unwrap();
            scene.view_frame(&cr, WIDTH, HEIGHT);
        }
        drawn.flush();
        (allocated, drawn.data().unwrap().to_vec())
    };
    let ((with, drawn_with), (without, drawn_without)) = (zoomed_in(true), zoomed_in(false));
    assert_ne!(drawn_with, drawn_without, "zoomed in, the footprint draws");
    assert!(with <= without, "zoomed in, the footprint allocates nothing: {with} against {without}");
}

/// A footprint's volume profile is drawn from the bars the footprint has
/// already read, with nothing allocated for it: zoomed in, a footprint with
/// its profile on allocates exactly what one with it off does, its figures
/// written or not; zoomed out, still what a chart without one does.
#[test]
fn a_footprint_s_profile_allocates_nothing_of_its_own() {
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, WIDTH as i32, HEIGHT as i32).unwrap();
    let cr = cairo::Context::new(&surface).unwrap();
    let frame = |scene: FlowScene| allocs(&mut scene.without_legend(), |scene| scene.view_frame(&cr, WIDTH, HEIGHT));
    let footprint = || FlowScene::new(400, 10, &["volume", "footprint"], 400, 0);
    // Cairo's own path buffers, grown by more rectangles, may come out an
    // allocation either way: never more than without the profile.
    for numbers in [true, false] {
        let (with, without) = (frame(footprint().footprint_profile(numbers)), frame(footprint()));
        assert!(with <= without, "zoomed in, figures {numbers}: {with} against {without}");
    }
    let ticked = |scene: FlowScene| {
        allocs(&mut scene.without_legend(), |scene| {
            scene.trade();
            scene.view_frame(&cr, WIDTH, HEIGHT);
        })
    };
    let (with, without) = (ticked(footprint().footprint_profile(true)), ticked(footprint()));
    assert!(with <= without, "a trade, then a frame: {with} against {without}");
    assert_eq!(
        frame(FlowScene::new(1_000, 400, &["volume", "footprint"], 400, 0).footprint_profile(true)),
        frame(FlowScene::new(1_000, 400, &["volume"], 400, 0)),
        "zoomed out"
    );
}
