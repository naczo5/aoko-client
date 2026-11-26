# GPUI API notes (pinned revision)

Everything here was verified against the **fetched source** at the pinned revision, not recalled from memory. GPUI's API churns and much public material describes older releases; several widely-copied idioms do **not** exist here.

- Repository: `https://github.com/zed-industries/zed`
- Pinned rev: `552644414c580ed540f027f6439881ea5c88d02a`
- Local checkout: `~/.cargo/git/checkouts/zed-a70e2ad075855582/5526444/`
- Toolchain: stable **1.100.0** (zed's own `rust-toolchain.toml` pin; no nightly features needed)
- `crates/aoko-app` must depend on **both** `gpui` and `gpui_platform`.

How to re-verify any claim below:

```bash
D=~/.cargo/git/checkouts/zed-a70e2ad075855582/5526444
grep -rn "fn <name>" "$D/crates/gpui/src/"
```

## 1. Bootstrap - there is no `Application::new()`

`Application` is `pub struct Application(Rc<AppCell>)` with **no bare constructor**. It requires an explicit platform. Use the `gpui_platform` helpers:

```rust
fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        // configure, open windows
    });
}
```

| Function | Purpose |
| --- | --- |
| `gpui_platform::application()` | windowed app on the real platform |
| `gpui_platform::headless()` | headless app (`current_platform(true)`) |
| `gpui_platform::linux(WindowingModes)` | Linux app that may switch windowing modes |
| `gpui_platform::current_headless_renderer()` | offscreen renderer (needs `test-support`/`bench-support`) |

Constructors on `Application` (verified present): `with_platform`, `new_inaccessible(platform)`, `with_assets`, `with_restart_arguments`, `with_http_client`, `with_quit_mode`, `with_windowing`, `run`, `run_embedded`, `update`, `to_async`, `on_open_urls`, `on_reopen`, `background_executor`, `foreground_executor`, `text_system`, `path_for_auxiliary_executable`.

`Application::run` takes `FnOnce(&mut App)`.

## 2. Rendering a view

```rust
impl Render for MyView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement { ... }
}
```

- `Render: 'static + Sized` (defined in `element.rs:172`). `render` returns `impl IntoElement`, so every branch must produce one concrete type.
- `RenderOnce::render(self, window, cx) -> impl IntoElement` for single-use elements.
- Open a window: `cx.open_window(options, |window, cx| -> Entity<V>) -> anyhow::Result<WindowHandle<V>>` where `V: Render + 'static`.

```rust
let bounds = Bounds::centered(None, size(px(1260.), px(820.)), cx);
cx.open_window(
    WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
    |_window, cx| cx.new(|_| MyShell),
).expect("open window");
```

`WindowOptions` fields include `window_bounds`, `titlebar`, `focus`, `show`, `kind`, `is_movable`, `app_owns_titlebar_drag`, `inactive_frame_interval`, `window_decorations`, `app_id`.

- `TitlebarOptions { title, appears_transparent, traffic_light_position }`. `appears_transparent` hides the system titlebar on **macOS and Windows only**; on Linux use `WindowOptions::window_decorations` (`WindowDecorations::Client` for a custom-drawn chrome).
- `WindowDecorations::{Server, Client}` - `Client` is what a custom titlebar needs on Linux/Wayland.

## 3. Entity / App / Context

- `cx.new(|cx| MyView { .. }) -> Entity<MyView>` creates app-owned state.
- `entity.update(cx, |this, cx| { ... })`, `entity.read(cx)`.
- `cx.notify()` marks *this* entity dirty. **Scoped reactivity rule:** notify only entities whose displayed fields changed. A blanket notify on every telemetry patch causes full re-render storms.
- `cx.observe(&other, |this, other, cx| ...) -> Subscription`; `Subscription` is cancelled on drop, so `.detach()` to keep it.
- `Context<Self>` derefs to `App` for app-level services.

## 4. Prebuilt style helpers DO exist (verified by compiling)

`Styled` provides Tailwind-like helpers. Confirmed compiling on `Div`:

`size_full`, `w_full`, `h_full`, `p_4`, `px_2`, `gap_2`, `rounded_lg`, `border_1`, `rounded(px(12.))`, `flex`, `flex_col`, `flex_row`, `items_center`, `justify_between`, `justify_center`, `bg`, `text_sm`, `text_color`, `font_weight`, `opacity`, `line_height`, `truncate`, `text_ellipsis`, `cursor_pointer`.

Full `Styled` method list at this revision (from `styled.rs`): `style block flex grid hidden scrollbar_width whitespace_normal whitespace_nowrap text_ellipsis* text_overflow text_align text_left|center|right truncate line_clamp flex_col flex_col_reverse flex_row flex_row_reverse flex_1 flex_auto flex_initial flex_none flex_basis flex_grow flex_grow_0 flex_grow_1 flex_shrink flex_shrink_0 flex_shrink_1 flex_wrap* items_start|end|center|baseline|stretch self_* justify_start|end|center|between|around|evenly content_* aspect_ratio aspect_square bg border_dashed text_style text_color font_weight text_bg text_size text_xs|sm|base|lg|xl|2xl|3xl italic not_italic underline line_through text_decoration_* font_family font_features font line_height opacity grid_cols* grid_rows* col_* row_* debug debug_below`.

Spacing/rounding helpers are generated (they do not appear as literal `fn p_4`), so **grep will mislead you** - compile a probe instead.

## 5. Scroll containers

`overflow_y_scroll()` / `overflow_x_scroll()` are **not** on plain `Div`. They come from `StatefulInteractiveElement`, which requires an id:

```rust
div().id("module-scroll").size_full().overflow_y_scroll().child(..)
```

A `div()` without `.id()` will fail to compile with "no method named `overflow_y_scroll`".

## 6. Interactivity

- `.id("...")` promotes `Div` to `StatefulInteractiveElement`.
- `.on_click(|event, window, cx: &mut App| {})` — note `&mut App`, **not** `Context<Self>`.
- `.on_key_down(|event: &KeyDownEvent, window: &mut Window, cx: &mut App| ...)` — again `&mut App`. Inside, to mutate your view you must hold an `Entity<Self>` handle and call `.update(cx, ..)`.
- `.hover(|style| style.opacity(0.9))` takes a style closure.
- Also available: `on_mouse_down`, `on_mouse_up`, `on_scroll` (`on_scroll_wheel` etc.), `cursor_pointer`.

Keybind capture buttons therefore need `Entity<Self>` + `on_key_down` + `cx.update`, not a `Context` closure.

## 7. Text, images, icons

- `.child("literal")` and `.child(format!(..))` work - `&str`/`String` implement `IntoElement`.
- Images: `img(source).size(px(32.))` where `source: impl Into<ImageSource>` (asset path, `Asset`, or `SharedUri`).
- Icons: `svg().path("icon.svg").size(px(16.))`. Paths resolve through the registered `AssetSource`.
- Virtualised lists: `uniform_list("id", len, |range, window, cx| -> Vec<AnyElement> { .. }).h(px(200.))`; map rows with `.into_any_element()`.

## 8. Fonts

`TextSystem::add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()>` (`text_system.rs:302`). Access via `cx.text_system()` (`app.rs:2649`).

```rust
let text_system = cx.text_system();
text_system.add_fonts(vec![
    Cow::Borrowed(include_bytes!("../../assets/Manrope-Medium.ttf")),
    Cow::Borrowed(include_bytes!("../../assets/minecraftia.ttf")),
]).expect("install fonts");
```

Install fonts **before** opening windows. `add_fonts` invalidates cached font resolution and clears layout caches, so call it once during startup. Then set `.font_family("Manrope")` — verify the family name actually resolves; a silent fallback is invisible in code and obvious in a screenshot.

## 9. Assets

```rust
pub trait AssetSource: 'static + Send + Sync {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>>;
    fn list(&self, path: &str) -> Result<Vec<SharedString>>;
}
```

Register with `gpui_platform::application().with_assets(Assets)`. `()` is a no-op implementation. Back this with `include_bytes!` or `rust-embed` for the logo and wordlist.

## 10. Deterministic screenshot capture (best option)

`window.rs:3268` provides offscreen rendering:

```rust
#[cfg(any(test, feature = "test-support"))]
pub fn render_to_image(&self) -> anyhow::Result<image::RgbaImage>
```

For a scripted capture harness use `HeadlessAppContext` (`app/headless_app_context.rs`):

- `HeadlessAppContext::with_platform(text_system, asset_source, renderer_factory)`
- `.open_window::<V>(options, build_root)`
- `.capture_screenshot(window) -> Result<RgbaImage>`
- `.advance_clock(Duration)` for deterministic time
- `.run_until_parked()` to settle async work
- `.simulate_window_scale_factor_change(..)`

Renderer factory on Linux: `gpui_platform::current_headless_renderer()` (wraps `gpui_wgpu::WgpuHeadlessRenderer`). Requires the `test-support` feature on both `gpui` and `gpui_platform`.

This is preferred over `grim`/`import`/`PrintWindow`: it needs no compositor, no visible window, and no per-OS capture code, and it is deterministic under `advance_clock`.

Fallback for a *live* window: `xvfb-run -a` plus `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`, then `import -window root out.png`.

## 11. Known traps at this revision

1. `Application::new()` does not exist. Use `gpui_platform::application()`.
2. `h_flex()` / `v_flex()` are **not** in `gpui`; they live in the separate `ui` crate (`crates/ui/src/components/stack.rs`). Build them yourself as `div().flex().items_center()`.
3. `overflow_y_scroll` needs `.id(..)` first.
4. `on_click` / `on_key_down`/`on_mouse_*` closures receive `&mut App`, never `Context<Self>`.
5. `render_to_image` is feature-gated behind `test-support`; it is not in a default build.
6. `include_str!`-style relative asset paths resolve from the **source file**, so keep an `assets/` directory adjacent to the crate and reference it consistently.
7. Do not float the zed rev. Bumping it invalidates every note above.
8. `LD_LIBRARY_PATH` exported by the enclosing tool environment ships an old `libstdc++`; strip it (`unset LD_LIBRARY_PATH`) before building or running, or Vulkan loads fail with `GLIBCXX_3.4.32 not found`.
9. **Typing has two routes, and they are not interchangeable — this one cost a wrong conclusion once.** A focused view's input handler is installed at the end of every draw from `next_frame.input_handlers` (`gpui/src/window.rs:3917-3925`, `set_input_handler`), so it is live on **both** Linux backends, not Wayland only. Each backend then calls `replace_text_in_range(None, key_char)` on that handler *after* the app callback returns, provided it propagated (`gpui_linux/src/linux/x11/window.rs:1230-1243`, `.../wayland/window.rs:1622-1632`). So a handler-only text field does insert typed characters under Xvfb. What that fallback cannot do is **edit**: it only ever sees `key_char`, so Backspace, Delete, Left, Right, Home and End do nothing unless an `on_key_down` listener takes them. A text field therefore needs both routes, and the listener must call `cx.stop_propagation()` on every key it consumes, or one keystroke inserts its character twice — once from the listener, once from the fallback.
10. `Window::dispatch_keystroke` (the usual test entry point) runs the input handler itself, after propagating (`window.rs:6178-6201`), so a test that only dispatches keystrokes cannot tell the two routes in item 9 apart. To exercise the listener alone, feed `PlatformInput::KeyDown` through `Window::dispatch_event`: `TestPlatform` implements neither `handle_input` nor `replace_text_in_range`, so on the test platform only the listener can edit.
11. **A click test silently depends on scroll position.** `Window::hit_test` (`window.rs:1179-1193`) intersects each hitbox with its `content_mask` before testing the point, so a control scrolled below the viewport cannot be hit at all: the click lands nowhere and the handler never runs. Dispatching a click at an element's `debug_bounds` without scrolling it into view therefore reports "inert" for a control that works, and looks identical to one that is genuinely broken. This produced a false finding in this project, where four switches were reported dead because they were below the fold. Scroll with a real `ScrollEvent` until the element is visible, then click.
