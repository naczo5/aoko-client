---
name: design-aesthetic
description: >-
  Visual design, typography, color palette, atmospheric styling, and UI/UX
  aesthetic rules for Aoko. Covers TYPE-MOON inspired visual novel aesthetics
  (Witch on the Holy Night / Mahoyo, Tsukihime: A piece of blue glass moon),
  deep midnight backdrops, luminous cyan/blue glass, crimson/coral accents,
  editorial typography, and cinematic motion across the website, docs, WPF GUI,
  and overlays.
---

# Aoko Visual Design Aesthetic

## 1. Core Heritage & Creative Philosophy

Aoko is named directly after **Aoko Aozaki** (the Fifth Magician / *Miss Blue*), drawing its visual soul and art direction from TYPE-MOON's modern visual novels:
- **Witch on the Holy Night (*Mahoutsukai no Yoru* / *Mahoyo*):** Atmospheric night landscapes, deep indigo-purple starry skies, luminous night blossoms, quiet supernatural mystery, and cinematic emotional depth.
- **Tsukihime (*A piece of blue glass moon*):** Pristine celestial moonlight, ethereal "blue glass" transparency, subtle chromatic rim aberration (magenta/crimson accents), crystalline minimalism, and razor-sharp elegance.

```
       [ Deep Nocturne Void ]  (#020711 / #050B17 / #0A0B0F)
                 │
  ┌──────────────┴──────────────┐
  ▼                             ▼
[ Luminous Blue Glass ]     [ Celestial Moon & Crimson Rim ]
(#7F9EFF / #9DB0FF / #38BDF8)   (#C7625A / #F43F5E / #EDF3F7)
  │                             │
  └──────────────┬──────────────┘
                 ▼
     [ Cinematic Elegance ]
  (Fraunces + Manrope typography,
   frosted glass, hairline borders,
   whisper-soft ambient glow)
```

### Guiding Principles

1. **The Contrast of Light and Void:** High-contrast, crystal-clear elements resting on deep, rich abyssal backdrops. The background is never flat grey—it is an expansive midnight sky infused with subtle radial gradients of twilight indigo and sea-teal haze.
2. **"Blue Glass" Transparency (Glassmorphism Done Right):** Surfaces feel like polished dark glass exposed to moonlight—semi-translucent, subtly blurred, with whisper-light 1px hairline borders and delicate specular top highlights.
3. **Editorial Typographic Restraint:** Sophisticated serif headers paired with ultra-clean modern geometric sans for UI text. Wide tracking on all-caps labels, generous whitespace, and purposeful layout breathing room.
4. **Cinematic Atmosphere Over Clutter:** Every glow, gradient, and transition serves the nocturnal mood. No garish RGB rainbow spam, no generic dashboard overload, and no tacky neon tropes.

---

## 2. Color Palette & Design Tokens

### Primary Palette (Nocturne & Blue Glass)

| Token | Hex / Value | Description | Usage |
| :--- | :--- | :--- | :--- |
| `--bg-void` | `#020711` | Pure abyssal midnight black-blue | Base background, deep contrast floor |
| `--bg-night` | `#050B17` / `#0A0B0F` | Midnight sky base | Page background, root window fill |
| `--surface-glass` | `rgba(11, 29, 42, 0.76)` | Translucent ocean-night glass | Cards, dialogs, floating panels (`backdrop-filter: blur(16px)`) |
| `--surface-solid` | `#0B1D2A` / `#12141A` | Solid dark navy-slate | Base panel containers, dropdown popups |
| `--surface-raised` | `#10283A` / `#181B22` | Raised element surface | Sliders, inputs, inactive tracks |
| `--surface-border` | `rgba(142, 177, 204, 0.18)` | Delicate hairline border | 1px borders on glass cards and inputs |
| `--surface-border-strong` | `rgba(157, 176, 255, 0.38)` | Active / focused border | Hovered cards, focused inputs, active tabs |

### Accent & Luminescence Tokens

| Token | Hex / Value | Motif / Origin | Usage |
| :--- | :--- | :--- | :--- |
| `--accent-magic` | `#7F9EFF` | Magic Blue / Moonbeam | Primary CTAs, active switch tracks, key glow effects |
| `--accent-bright` | `#9DB0FF` | Electric Starfire | Inline code highlights, hovered links, button text |
| `--accent-deep` | `#2B3A86` | Twilight Leyline Indigo | Background radial shadows, deep button gradients |
| `--accent-flora` | `#38BDF8` / `#8EC9AC` | Mahoyo Meadow Cyan / Mint | Success states, special module tags, ethereal indicators |
| `--accent-crimson` | `#C7625A` / `#F43F5E` | Tsukihime Rim / Aoko Coral | Focus highlights, sliders, keybind badges, combat accents |
| `--accent-crimson-deep`| `rgba(199, 98, 90, 0.22)`| Coral Ember Veil | Subtle crimson background glow, warning states |

### Text & Starlight Tokens

| Token | Hex / Value | Description | Usage |
| :--- | :--- | :--- | :--- |
| `--text-starlight` | `#EDF3F7` / `#E8EAEE` | Luminous crisp silver-white | Primary headings, body copy, active labels |
| `--text-muted` | `#9CAFBC` / `#7A8290` | Misty ambient blue-grey | Secondary descriptions, subheadings, subtitles |
| `--text-dim` | `#698092` / `#505A69` | Distant twilight shadow | Inactive placeholders, shortcut keys, disabled text |

---

## 3. Typography Hierarchy & Styling Rules

```
DISPLAY SERIF    Fraunces / Zen Old Mincho / Cinzel
                 "aoko client"  ──  "T S U K I H I M E"
                 (Lyrical, high-contrast, editorial elegance)
                        │
SANS-SERIF       Manrope / Inter / Segoe UI
                 "A free, open-source utility client..."
                 (Crisp, legible, balanced weight 400-600)
                        │
MONOSPACE        JetBrains Mono / Fira Code
                 "scoop bucket add aoko"  ──  [ LSHIFT ]
                 (Glass pill badges, terminal snippets, binds)
```

### Font Pairings

- **Primary Display / Titling:** `Fraunces` (variable optical size 9–144, weights 500–700) or high-contrast editorial serifs (`Cinzel`, `Cormorant Garamond`, `Zen Old Mincho` / `Noto Serif JP`).
- **Interface & Body:** `Manrope` (weights 400, 500, 600, 700) or clean modern sans (`Inter`, `Segoe UI`, `system-ui`).
- **Code & Keybind Badges:** `Manrope` / `JetBrains Mono` / `Fira Code`.

### Typographic Rules

1. **Title Case & Spacing:**
   - Visual Novel Subtitles & Eyebrow Badges: Use wide letter-spacing (`letter-spacing: 0.12em` to `0.25em`), uppercase, font size `0.75rem`–`0.85rem` (e.g. `A PIECE OF BLUE GLASS MOON`, `ARCHITECTURE OVERVIEW`).
   - Brand Logo: Understated lowercase (`aoko` or `aoko client`) with high-contrast serif font weight.
2. **Line Heights & Readability:**
   - Body copy line height: `1.6`–`1.8` for effortless readability against dark backgrounds.
   - Headings: Tight, balanced line height (`1.1`–`1.25`) with slight negative letter-spacing (`-0.02em`) on large display text.
3. **Contrast:**
   - Never use `#FFFFFF` on pitch black for dense text (causes eye fatigue). Use `--text-starlight` (`#EDF3F7`) with crisp font smoothing (`-webkit-font-smoothing: antialiased`).

---

## 4. Surfaces, Glassmorphism & Atmospheric Lighting

### The "Blue Glass" Card Formula

When building cards, modal panels, or floating windows:

```css
.glass-panel {
  background: var(--surface-glass); /* rgba(11, 29, 42, 0.76) */
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
  border: 1px solid var(--surface-border); /* rgba(142, 177, 204, 0.18) */
  border-radius: 12px;
  box-shadow:
    inset 0 1px 0 0 rgba(255, 255, 255, 0.08),  /* Specular top rim light */
    0 8px 32px 0 rgba(2, 7, 17, 0.45);          /* Deep ambient drop shadow */
  transition: all 0.25s cubic-bezier(0.16, 1, 0.3, 1);
}

.glass-panel:hover {
  border-color: var(--surface-border-strong);
  box-shadow:
    inset 0 1px 0 0 rgba(255, 255, 255, 0.14),
    0 12px 40px 0 rgba(127, 158, 255, 0.12);    /* Soft lunar back-glow */
  transform: translateY(-2px);
}
```

### Atmospheric Depth Layering

Layer web pages and visual presentations using depth planes:

1. **Backdrop Plane (Z: -3):** Deep gradient canvas with midnight-navy to void (`linear-gradient(180deg, #050d1c 0%, #050b17 42%, #020711 100%)`).
2. **Nebula / Atmospheric Haze (Z: -2):** Soft radial gradients:
   - Upper right / center: Radial glow in twilight indigo (`rgba(43, 58, 134, 0.28)`).
   - Mid left / horizon: Subtle aquatic teal / misty glow (`rgba(32, 84, 93, 0.16)`).
3. **Celestial Plane (Z: -1):** Moon, parallax starlight field, and floating moon-dust particles.
4. **Foreground Silhouettes / Flora:** Night forest layers or glowing nemophila/star flower silhouettes.
5. **Interactive UI Plane (Z: 1+):** Frosted glass panels, typography, and controls.

---

## 5. UI Components & Platform Guidelines

### A. Web Landing Page & Documentation (`website/`)

- **Hero Composition:** Cinematic framing with a celestial moon or starry sky, elegant headline, quick-action CTA button, and terminal preview pill.
- **Buttons:**
  - *Primary CTA:* Gradient fill (`linear-gradient(135deg, #7f9eff, #5d80e6)`), dark text (`#050b17`), crisp rounded corners (`8px`), soft luminous shadow (`box-shadow: 0 4px 20px rgba(127, 158, 255, 0.35)`).
  - *Secondary / Ghost Button:* Transparent fill, border `1px solid var(--line)`, starlight text (`#edf3f7`), subtle blue hover sheen.
- **Code & Terminal Snippets:**
  - Mac/terminal header bar with subtle red/yellow/green starlight dots.
  - Background `rgba(8, 20, 31, 0.92)` with crisp hairline border and starlight prompt markers (`PS C:\>`).

### B. WPF Desktop Loader (`Aoko/App.xaml`, `MainWindow.xaml`)

- **Root Window:** `#0A0B0F` solid background with `#12141A` inner card panels.
- **Accent Theme:** Coral/crimson accent (`#C7625A`) paired with slate-indigo fills (`#181B22`, `#2A2F38`).
- **Sliders (`DarkSlider`):**
  - Track: Thin, rounded `#181B22` bar.
  - Active fill: Accent brush (`#C7625A` or `#7F9EFF`).
  - Thumb: Sleek vertical pill with subtle drop shadow that scales smoothly on hover/drag (`ScaleX=1.4, ScaleY=1.15`).
- **Module Switches (`ModuleSwitch`):**
  - Pill track with smooth spring animation on toggle.
  - Thumb transitions smoothly to white on active state with accent glow.
- **Scrollbars:** Ultra-thin (4px) minimalist track with glowing accent thumb (`CornerRadius="2"`, subtle drop shadow).

### C. In-Game Overlay & ImGui HUD (`McInjector/`)

- **Minimalist & Distraction-Free:** High contrast against Minecraft world textures without blocking vision.
- **Colors:** Deep translucent black-blue background (`rgba(5, 11, 23, 0.82)`), clean white text, and customizable accent highlights (Moonlit Cyan or Coral Crimson).
- **Typography:** Crisp bitmap/TrueType font rendering with subtle drop shadow for readability against snow, sky, and sun.

### D. Agent Artifacts & Presentations

- **Theme Consistency:** When writing markdown artifacts, documentation, or design specs, format tables, alerts, and mermaid diagrams to harmonize with this aesthetic.
- **Mermaid Diagrams:** Use dark mode configuration with sapphire nodes, cyan edges, and crimson highlight states:
  ```mermaid
  %%{init: {'theme': 'dark', 'themeVariables': { 'primaryColor': '#10283A', 'primaryTextColor': '#EDF3F7', 'primaryBorderColor': '#7F9EFF', 'lineColor': '#9DB0FF', 'secondaryColor': '#0B1D2A', 'tertiaryColor': '#050B17' }}}%%
  graph TD
      A[Nocturne Sky] --> B[Blue Glass Moon]
      B --> C[Luminous Interface]
  ```

---

## 6. Motion & Micro-Interactions

| Interaction | Duration | Timing Curve | Visual Behavior |
| :--- | :--- | :--- | :--- |
| **Card Hover Lift** | `200ms–250ms` | `cubic-bezier(0.16, 1, 0.3, 1)` | `translateY(-2px)`, border lightens, soft ambient glow expands |
| **Button Press** | `80ms` | `ease-out` | `scale(0.96)` for tactile tactile response |
| **Toggle Switch** | `200ms` | `ease-out` | Thumb slides horizontally with easing, track opacity sweeps |
| **Slider Grab** | `120ms` | `ease-out` | Thumb scales up to `1.4x` width, glowing halo expands |
| **Celestial Parallax**| Continuous | Smooth requestAnimationFrame | Subtle moon / starfield offset linked to scroll position |

---

## 7. Forbidden Clichés & Anti-Patterns

❌ **DO NOT:**
1. **Generic Purple-on-Dark ("AI Neon"):** Avoid generic saturated purple/magenta grids, neon synthwave pinks, or violet overload. Stay anchored in midnight navy, deep sapphire, starlight white, and subtle coral/crimson rims.
2. **Chunky, High-Contrast Borders:** Never use solid 2px+ white or bright colored borders. Always use semi-transparent 1px hairlines (`rgba(142, 177, 204, 0.18)`).
3. **Over-Cluttered Dashboard Bento Boxes:** Avoid filling every pixel with unrelated stat boxes, arbitrary badges, and decorative fluff. Embrace intentional whitespace.
4. **Flat Grey-on-Black:** Avoid sterile `#111111` or `#1E1E1E` without depth. Layer rich navy-slate tones and ambient radial lighting.
5. **Noisy Textures & Heavy Drop Shadows:** Avoid grainy retro noise overlays or harsh solid drop shadows. Use diffused atmospheric ambient shadows.

---

## 8. Verification Checklist

When building or reviewing any visual surface in Aoko:

- [ ] **Palette Harmony:** Primary colors adhere to midnight navy (`#050B17`), blue glass (`#7F9EFF`), starlight (`#EDF3F7`), and coral/crimson accents (`#C7625A` / `#F43F5E`).
- [ ] **Typography:** Display serif or tracked uppercase headers paired with crisp sans body copy.
- [ ] **Glassmorphism:** Frosted panels use semi-transparency, blur, 1px hairlines, and top specular highlights.
- [ ] **Contrast & Legibility:** Text is effortlessly legible on dark surfaces without harsh blinding white glare.
- [ ] **Micro-Interactions:** Hover states and transitions are smooth, tactile, and fast (<250ms).
- [ ] **Atmospheric Depth:** Backgrounds possess depth (radial light, subtle starlight, moonlight haze).
- [ ] **Platform Consistency:** Visual identity feels unified across website, docs, WPF GUI, and overlays.
