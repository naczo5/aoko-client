/* aoko client — preview GUI data model.
 * Mirrors Aoko/MainWindow.xaml so the in-browser mock stays faithful to the
 * real external GUI. Consumed by preview.js.
 *
 * Control types:
 *   { type:'slider',  label, min, max, value, step, decimals, gate }
 *   { type:'checks',  items:[{label, checked}], gate }
 *   { type:'check',   label, checked, gate }
 *   { type:'switch',  label, on, main, gate }   // full-width toggle row
 *   { type:'select',  label, options:[...], index, gate }
 *   { type:'button',  label }
 *   { type:'buttons', label, items:[...] }       // inline row of buttons
 *   { type:'note',    text, warn }
 *   { type:'keybinds', items:[...] }
 *   { type:'palettes' }                          // Settings only
 *   { type:'modstyles' }                         // Settings only
 *
 * Card shape:
 *   { title, toggle:{on,main}|null, status:{armed,available}|text|null,
 *     desc, controls:[...] }
 */
window.AOKO_PREVIEW_DATA = {
  palettes: {
    Slate:    { bg:'#0A0B0F', panel:'#12141A', line:'#181B22', accent:'#C7625A', text:'#E8EAEE' },
    Ink:      { bg:'#08090B', panel:'#101115', line:'#16181C', accent:'#B0B6C0', text:'#E8EAEE' },
    Graphite: { bg:'#0B0B0D', panel:'#131316', line:'#19191C', accent:'#B89B82', text:'#E8EAEE' },
    Steel:    { bg:'#08090C', panel:'#0F1218', line:'#161A21', accent:'#6B8DAB', text:'#E8EAEE' },
  },
  defaultPalette: 'Slate',
  moduleStyles: ['Default', 'Minimal', 'Outlined', 'Glass', 'Bold'],
  defaultModuleStyle: 'Default',

  tabs: [
    {
      id: 'combat', label: 'Combat',
      cards: [
        {
          title: 'AutoClicker',
          toggle: { on: true, main: true },
          status: { armed: 'Armed', available: 'Disarmed' },
          controls: [
            { type: 'slider', label: 'Min CPS', min: 1, max: 25, value: 12, step: 1, decimals: 0 },
            { type: 'slider', label: 'Max CPS', min: 1, max: 25, value: 16, step: 1, decimals: 0 },
            { type: 'checks', items: [
              { label: 'Jitter', checked: true },
              { label: 'Click in Chests', checked: true },
              { label: 'Pause when breaking', checked: true },
            ] },
          ],
        },
        {
          title: 'Right Click',
          controls: [
            { type: 'switch', label: 'Enable Right Click', on: false, main: true },
            { type: 'slider', label: 'Right Min CPS', min: 1, max: 25, value: 13, step: 1, decimals: 0 },
            { type: 'slider', label: 'Right Max CPS', min: 1, max: 25, value: 16, step: 1, decimals: 0 },
            { type: 'check', label: 'Only when holding block', checked: true },
          ],
        },
        {
          title: 'Aim Assist',
          toggle: { on: true, main: true },
          status: 'Available',
          controls: [
            { type: 'slider', label: 'FOV', min: 1, max: 180, value: 45, step: 1, decimals: 0, gate: true },
            { type: 'slider', label: 'Range', min: 1, max: 12, value: 3, step: 0.1, decimals: 1, gate: true },
            { type: 'slider', label: 'Strength (%)', min: 1, max: 100, value: 50, step: 1, decimals: 0, gate: true },
          ],
        },
        {
          title: 'Triggerbot',
          toggle: { on: false, main: true },
          status: 'Available',
          controls: [
            { type: 'slider', label: 'Cooldown Threshold (%)', min: 1, max: 100, value: 80, step: 1, decimals: 0, gate: true },
            { type: 'check', label: 'Only attack when crosshair is on entity', checked: true, gate: true },
            { type: 'check', label: 'Only attack if target is in real hit range', checked: false, gate: true },
            { type: 'slider', label: 'Hit Chance (%)', min: 1, max: 100, value: 100, step: 1, decimals: 0, gate: true },
            { type: 'check', label: 'Only while holding left click', checked: false, gate: true },
          ],
        },
      ],
    },

    {
      id: 'render', label: 'Render',
      cards: [
        {
          title: 'Nametags',
          toggle: { on: true, main: true },
          controls: [
            { type: 'checks', gate: true, items: [
              { label: 'Show Health', checked: true },
              { label: 'Show Armor', checked: true },
              { label: 'Hide Vanilla Tags', checked: true },
              { label: 'Show Item', checked: false },
            ] },
            { type: 'slider', label: 'Nametag Count', min: 1, max: 20, value: 10, step: 1, decimals: 0, gate: true },
          ],
        },
        {
          title: 'Chest ESP',
          toggle: { on: true, main: true },
          controls: [
            { type: 'slider', label: 'Chest Count', min: 1, max: 20, value: 10, step: 1, decimals: 0, gate: true },
          ],
        },
        {
          title: 'Block ESP (X-ray)',
          toggle: { on: false, main: true },
          controls: [
            { type: 'checks', gate: true, items: [
              { label: 'Boxes', checked: true },
              { label: 'Tracers', checked: false },
              { label: 'HUD List', checked: true },
            ] },
            { type: 'slider', label: 'Max Blocks', min: 1, max: 512, value: 256, step: 1, decimals: 0, gate: true },
            { type: 'slider', label: 'Scan Range (chunks)', min: 1, max: 8, value: 4, step: 1, decimals: 0, gate: true },
            { type: 'note', text: 'Tracked blocks (e.g. minecraft:diamond_ore) are managed in a scrollable list in the app.' },
          ],
        },
        {
          title: 'Closest Player',
          toggle: { on: true, main: true },
          controls: [
            { type: 'note', text: 'Includes 4-way direction arrow (^, v, <, >).' },
          ],
        },
        {
          title: 'Show module list (top-right)',
          toggle: { on: true, main: true },
          controls: [
            { type: 'note', text: 'Shows/hides the active modules overlay list in-game.' },
          ],
        },
        {
          title: 'Edit HUD',
          toggle: { on: false, main: true },
          controls: [
            { type: 'note', text: 'Drag HUD elements in-game to reposition them. Open chat to move things around.' },
            { type: 'button', label: 'Reset layout' },
          ],
        },
        {
          title: 'AntiDebuff',
          status: 'Available',
          controls: [
            { type: 'switch', label: 'Enable AntiDebuff', on: false, main: true },
            { type: 'note', text: 'Removes debuff visuals client-side (Blindness, Nausea, and Darkness on modern versions). The server still considers you affected.' },
          ],
        },
      ],
    },

    {
      id: 'utility', label: 'Utility',
      cards: [
        {
          title: 'SpeedBridge',
          toggle: { on: false, main: true },
          status: 'Available',
          controls: [
            { type: 'slider', label: 'Safety', min: 20, max: 250, value: 150, step: 1, decimals: 0, gate: true },
            { type: 'checks', gate: true, items: [
              { label: 'Block only', checked: true },
              { label: 'Holding sneak only', checked: true },
              { label: 'Looking down only', checked: true },
            ] },
          ],
        },
        {
          title: 'Discord Rich Presence',
          toggle: { on: true, main: true },
          controls: [
            { type: 'note', text: 'Shows your current client version, armed/clicking state, and in-game status on Discord.' },
            { type: 'note', text: 'The Discord activity includes a Repository button linking to this project.' },
          ],
        },
        {
          title: 'GTB Helper',
          toggle: { on: false, main: true },
          controls: [
            { type: 'note', text: 'Hypixel Guess The Build helper using action-bar hints.' },
          ],
        },
        {
          title: 'Pixel Party Assist',
          toggle: { on: false, main: true },
          controls: [
            { type: 'note', text: 'Hypixel Pixel Party: finds the closest matching terracotta on the floor.' },
            { type: 'slider', label: 'Scan radius (blocks)', min: 8, max: 48, value: 24, step: 1, decimals: 0, gate: true },
            { type: 'check', label: 'Auto-look toward target', checked: false, gate: true },
            { type: 'check', label: 'Auto-walk toward target (W + jump far, walk close)', checked: false, gate: true },
          ],
        },
        {
          title: 'Chest Stealer',
          status: 'Available',
          controls: [
            { type: 'switch', label: 'Enable Chest Stealer', on: false, main: true },
            { type: 'slider', label: 'Delay (ms)', min: 50, max: 500, value: 120, step: 10, decimals: 0 },
          ],
        },
        {
          title: 'Panic',
          controls: [
            { type: 'note', text: 'Immediately disables modules, hides UI/overlay settings, disconnects, and closes the loader. Best-effort only.' },
            { type: 'button', label: 'Trigger Panic Now' },
            { type: 'note', text: "You can also bind Panic in Module Keybinds." },
          ],
        },
        {
          title: 'Module Keybinds',
          controls: [
            { type: 'note', text: 'Click a button, then press a key (Esc = unbind).' },
            { type: 'keybinds', items: [
              'Autoclicker', 'Right Click', 'Jitter', 'Click in Chests', 'Break Blocks',
              'Aim Assist', 'Triggerbot', 'SpeedBridge', 'GTB Helper', 'Pixel Party',
              'Nametags', 'Chest ESP', 'Chest Stealer', 'Block ESP', 'Closest Player',
              'Reach', 'Velocity', 'AutoTotem', 'AntiDebuff', 'Panic',
            ] },
          ],
        },
      ],
    },

    {
      id: 'risky', label: 'Risky',
      cards: [
        {
          title: 'Reach',
          status: 'Available',
          controls: [
            { type: 'switch', label: 'Enable Reach', on: false, main: true },
            { type: 'slider', label: 'Min Range', min: 3, max: 6, value: 3, step: 0.1, decimals: 1 },
            { type: 'slider', label: 'Max Range', min: 3, max: 6, value: 3, step: 0.1, decimals: 1 },
            { type: 'slider', label: 'Chance (%)', min: 0, max: 100, value: 100, step: 1, decimals: 0 },
          ],
        },
        {
          title: 'Velocity',
          status: 'Available',
          controls: [
            { type: 'note', text: 'idk if you should use it outside of singleplayer' },
            { type: 'switch', label: 'Enable Velocity', on: false, main: true },
            { type: 'slider', label: 'Horizontal (%)', min: 1, max: 100, value: 100, step: 1, decimals: 0 },
            { type: 'slider', label: 'Vertical (%)', min: 1, max: 100, value: 100, step: 1, decimals: 0 },
            { type: 'slider', label: 'Chance (%)', min: 1, max: 100, value: 100, step: 1, decimals: 0 },
          ],
        },
        {
          title: 'AutoTotem',
          status: 'Available',
          controls: [
            { type: 'switch', label: 'Enable AutoTotem', on: false, main: true },
            { type: 'select', label: 'Mode', options: ['Smart (health + elytra)', 'Strict (always hold)'], index: 0 },
            { type: 'slider', label: 'Health Threshold', min: 0, max: 36, value: 18, step: 1, decimals: 0 },
            { type: 'slider', label: 'Delay (ticks)', min: 0, max: 20, value: 0, step: 1, decimals: 0 },
            { type: 'switch', label: 'Elytra', on: false },
            { type: 'select', label: 'Behavior', options: ['Ghost (inventory only)', 'Anarchy (may flag on servers)'], index: 0 },
          ],
        },
      ],
    },

    {
      id: 'settings', label: 'Settings',
      cards: [
        {
          title: 'Configs',
          controls: [
            { type: 'note', text: 'Save the current settings as a named config, then switch between configs at any time. Configs are stored as .json files.' },
            { type: 'select', label: 'Select config', options: ['default'], index: 0 },
            { type: 'buttons', items: ['Load', 'Save', 'Delete'] },
            { type: 'button', label: 'Open Configs Folder' },
          ],
        },
        {
          title: 'Reload Mappings',
          status: 'Available',
          controls: [
            { type: 'note', text: 'Use this if something is not working properly (missing overlays, broken nametags, stale in-game data after injection).' },
            { type: 'button', label: 'Reload Mappings' },
          ],
        },
        {
          title: 'GUI Customization',
          controls: [
            { type: 'note', text: 'External GUI Palette — click a palette to apply it instantly.' },
            { type: 'palettes' },
            { type: 'note', text: 'Module List Style' },
            { type: 'modstyles' },
            { type: 'switch', label: "Show 'aoko' Logo", on: true },
          ],
        },
      ],
    },
  ],
};
