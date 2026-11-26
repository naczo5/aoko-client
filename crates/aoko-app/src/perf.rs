//! Deterministic allocation accounting for the UI layer.
//!
//! `MIGRATION_ARCHITECTURE.md` section 6 binds the UI to: *no per-frame cloning or style
//! rebuilding in `Render::render`; static tokens precomputed; filtered lists memoized;
//! `cx.notify()` scoped to entities whose displayed fields changed.* Allocation counts
//! are deterministic and frame times are not, so this module counts allocations and
//! never measures a duration.
//!
//! # Why the counter is a global allocator
//!
//! A `#[global_allocator]` is the only way to see allocations made by code that never
//! calls an allocator explicitly — `format!`, `Vec`, `Rc`, a `String` grown by push, a
//! closure boxed by `gpui`'s listener API. It is installed in test builds only, counts
//! per thread, and is a no-op in the shipped binary: the whole module is behind
//! `#[cfg(test)]` and the crate root never names it outside tests. The counter itself
//! allocates nothing — a `Cell<usize>` in a `const`-initialised thread-local, which has
//! no lazy initialiser and no destructor to register — because an allocator that
//! allocates while counting allocations would not terminate.
//!
//! # What a number means
//!
//! [`count`] reports allocations on the calling thread between two points, so a test
//! brackets one `Render::render` call and reads the number of heap allocations that
//! frame made. Frees are not counted: a frame that allocates and immediately drops is
//! still a frame that allocated.
//!
//! # Why there is no bench target
//!
//! This crate adds no bench target. A bench measures
//! durations, and the clause is about counts: the counting harness above is the
//! deterministic measure and it runs on every `cargo test`. A duration measured against
//! `TestPlatform` — the only platform a headless bench can build — would time gpui's
//! test double rather than a real frame, and the number would move with the machine.
//! A flaky bench that no gate reads is worse than the assertions this module already
//! makes.

#[cfg(test)]
pub use counting::{allocations, count};

/// The counting allocator, and the guard that makes it test-only.
///
/// `System` remains the real allocator; this only increments a counter first. `realloc`
/// counts as an allocation because it is one (the pointer may move), and `alloc_zeroed`
/// is forwarded so a caller asking for zeroed memory still gets it.
#[cfg(test)]
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    thread_local! {
        /// Allocations made on this thread. `const`-initialised, so reading it cannot
        /// allocate and the thread-local has no destructor to run at thread exit.
        static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    }

    fn record() {
        // `try_with` rather than `with`: during thread teardown the slot can already be
        // gone, and an allocator that panics there would abort the whole test binary.
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
    }

    pub struct CountingAllocator;

    // SAFETY: every method forwards to `System` with the arguments it was given, so the
    // global allocator's contract is `System`'s contract. The only added work is a
    // thread-local increment, which neither allocates nor unwinds.
    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            record();
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            record();
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            record();
            unsafe { System.realloc(ptr, layout, new_size) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    /// Allocations on this thread since it started.
    #[must_use]
    pub fn allocations() -> usize {
        ALLOCATIONS.try_with(Cell::get).unwrap_or(0)
    }

    /// Runs `f` and returns its value with the allocations it made on this thread.
    pub fn count<T>(f: impl FnOnce() -> T) -> (T, usize) {
        let before = allocations();
        let value = f();
        (value, allocations() - before)
    }
}

/// The process-wide counter, in test builds only.
#[cfg(test)]
#[global_allocator]
static ALLOCATOR: self::counting::CountingAllocator = self::counting::CountingAllocator;

#[cfg(test)]
mod tests {
    use super::count;
    use crate::shell::Shell;
    use crate::theme::Theme;
    use gpui::{
        AppContext as _, Entity, Render as _, TestAppContext, VisualTestContext, WindowOptions,
    };

    /// The steady-state allocation count for a shell frame on the default tab.
    ///
    /// Measured, not guessed: this is what `Shell::render` does on the second and every
    /// later frame with the Combat tab showing, after the ids, the chrome strings and
    /// every derived label were memoised. The number is the pin's element model, not
    /// ours: a bare `div()` is 2 allocations and `into_any_element()` is 4 more (a boxed
    /// element object), `on_click` is an `Rc` plus a `Vec` growth (`div.rs:619`), and
    /// `hover`/`active` are two more boxed closures each. A frame with zero allocations
    /// is not reachable through the pin's builder API without writing raw `Element`
    /// impls, which is a redesign rather than a fix.
    ///
    /// Treat this as a budget: if a change to the UI legitimately moves it, update the
    /// constant in the same commit and say why. If it moves on its own, something in
    /// `render` started allocating.
    const SHELL_FRAME_ALLOCATIONS: usize = 1006;

    fn warm_shell() -> (VisualTestContext, Entity<Shell>) {
        let cx = TestAppContext::single();
        let shell = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, _| shell.clone())
                .expect("test window")
        });
        let mut visual = VisualTestContext::from_window(window.into(), &cx);
        // A full frame registers the input handlers and creates the text fields; the
        // first render also fills every memo. Measurement starts after it.
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        (visual, shell)
    }

    /// One `Shell::render`, with the tree dropped so the next call starts level.
    fn render_once(visual: &mut VisualTestContext, shell: &Entity<Shell>) {
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let element = shell.render(window, cx);
                drop(element);
            });
        });
    }

    #[test]
    fn a_steady_state_shell_frame_stays_within_its_allocation_budget() {
        let (mut visual, shell) = warm_shell();

        // Warm up: the first post-draw render fills the caches for this tab.
        render_once(&mut visual, &shell);

        let (_, first) = count(|| render_once(&mut visual, &shell));
        let (_, second) = count(|| render_once(&mut visual, &shell));

        assert_eq!(
            first, second,
            "two identical frames must cost the same; a difference means a cache grows \
             or state accumulates between frames"
        );
        assert_eq!(
            first, SHELL_FRAME_ALLOCATIONS,
            "steady-state Shell::render allocations: check the budget comment above"
        );
    }

    /// The other tabs, so a regression on a tab the default test never draws is caught.
    #[test]
    fn every_tab_stays_within_the_same_budget() {
        let (mut visual, shell) = warm_shell();

        for tab in 0..9 {
            visual.update(|_, cx| shell.update(cx, |shell, cx| shell.select_tab(tab, cx)));
            render_once(&mut visual, &shell);
            let (_, allocations) = count(|| render_once(&mut visual, &shell));
            assert!(
                allocations <= TAB_BUDGET,
                "tab {tab} allocated {allocations}, over the {TAB_BUDGET} budget"
            );
        }
    }

    /// The largest per-tab count measured, with headroom for the pin's own element cost.
    ///
    /// The tabs differ by how many controls they draw (Combat ≈ 1000, Stats ≈ 150), so
    /// one ceiling over all nine catches a regression anywhere without pinning nine
    /// numbers that any copy change would move.
    const TAB_BUDGET: usize = 2228;
}

#[cfg(test)]
mod notify_tests {
    use crate::shell::Shell;
    use crate::theme::Theme;
    use gpui::{AppContext as _, TestAppContext};
    use std::cell::Cell;
    use std::rc::Rc;

    /// Section 6's last clause: "`cx.notify()` scoped to entities whose displayed fields
    /// changed".
    ///
    /// The shell is one entity for every control, so the scope here is "did any field it
    /// paints change". A slider drag delivers repeats of the same value at the ends of
    /// its travel and a checkbox press can land on the state it already shows; neither
    /// changes a pixel, so neither may repaint. The observer fires on `notify`, which is
    /// the only public way to see it at the pin.
    #[test]
    fn setting_a_control_to_the_value_it_already_shows_does_not_notify() {
        let cx = TestAppContext::single();
        let shell = cx.update(|cx| cx.new(|_| Shell::new(Theme::slate())));

        let notifications = Rc::new(Cell::new(0_usize));
        let counter = notifications.clone();
        cx.update(|cx| {
            cx.observe(&shell, move |_, _| counter.set(counter.get() + 1))
                .detach();
        });

        // A real change notifies once.
        cx.update(|cx| shell.update(cx, |shell, cx| shell.set_value("MinCPS", 12.0, cx)));
        cx.run_until_parked();
        let after_change = notifications.get();
        assert_eq!(after_change, 1, "a new value repaints");

        // The same value again, which is what a drag at the end of its travel sends.
        cx.update(|cx| shell.update(cx, |shell, cx| shell.set_value("MinCPS", 12.0, cx)));
        cx.run_until_parked();
        assert_eq!(
            notifications.get(),
            after_change,
            "an unchanged slider value must not repaint the shell"
        );

        // Same rule for a checkbox.
        cx.update(|cx| shell.update(cx, |shell, cx| shell.set_checked("ClickInChests", true, cx)));
        cx.run_until_parked();
        let after_check = notifications.get();
        assert_eq!(
            after_check,
            after_change + 1,
            "a new checkbox state repaints"
        );
        cx.update(|cx| shell.update(cx, |shell, cx| shell.set_checked("ClickInChests", true, cx)));
        cx.run_until_parked();
        assert_eq!(
            notifications.get(),
            after_check,
            "re-setting the checkbox state must not repaint"
        );
    }
}
