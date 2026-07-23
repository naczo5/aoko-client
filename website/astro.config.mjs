// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// The site is published to GitHub Pages at https://naczo5.github.io/aoko-client/
// The hand-rolled landing page lives in `public/` and keeps the site root ("/aoko-client/").
// Starlight owns the documentation routes underneath it (e.g. /getting-started/, /modules/...).
export default defineConfig({
  site: 'https://naczo5.github.io',
  base: '/aoko-client',
  trailingSlash: 'always',
  integrations: [
    starlight({
      title: 'aoko docs',
      description:
        'Documentation for aoko client — an open-source utility client for Lunar Client and standalone Minecraft instances (1.8.9 Forge, modern Fabric).',
      tagline: 'Open-source utility client for Lunar Client & standalone Minecraft instances (1.8.9, 1.21.x, 26.1, 26.2).',
      // Link the docs back to the landing page (served at the site root).
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/naczo5/aoko-client',
        },
      ],
      // A small link back to the marketing landing page that lives in public/.
      components: {},
      sidebar: [
        {
          label: 'Basics',
          items: [
            { label: 'Getting started', slug: 'getting-started' },
            { label: 'Profiles & keybinds', slug: 'profiles-keybinds' },
            { label: 'Panic', slug: 'panic' },
          ],
        },
        {
          label: 'Modules',
          items: [
            {
              label: 'Combat',
              items: [
                { label: 'Kill Aura', slug: 'modules/combat/killaura' },
                { label: 'Autoclicker', slug: 'modules/combat/autoclicker' },
                { label: 'Aim Assist', slug: 'modules/combat/aim-assist' },
                { label: 'Triggerbot', slug: 'modules/combat/triggerbot' },
                { label: 'Reach', slug: 'modules/combat/reach' },
                { label: 'Velocity', slug: 'modules/combat/velocity' },
                { label: 'AutoTotem', slug: 'modules/combat/autototem' },
              ],
            },
            {
              label: 'Movement',
              items: [
                { label: 'SpeedBridge', slug: 'modules/movement/speedbridge' },
                { label: 'PixelParty Assist', slug: 'modules/movement/pixelparty-assist' },
              ],
            },
            {
              label: 'Visual',
              items: [
                { label: 'Nametags', slug: 'modules/visual/nametags' },
                { label: 'Closest Player', slug: 'modules/visual/closest-player' },
                { label: 'Fight Status', slug: 'modules/visual/fight-status' },
                { label: 'Chest ESP', slug: 'modules/visual/chest-esp' },
                { label: 'Block ESP', slug: 'modules/visual/block-esp' },
                { label: 'Module List & Logo', slug: 'modules/visual/module-list' },
                { label: 'HUD Editor', slug: 'modules/visual/hud-editor' },
              ],
            },
            {
              label: 'Utility',
              items: [
                { label: 'Nick Hider', slug: 'modules/utility/nick-hider' },
                { label: 'Hit Delay Fix', slug: 'modules/utility/hit-delay-fix' },
                { label: 'Chest Stealer', slug: 'modules/utility/chest-stealer' },
                { label: 'GTB Helper', slug: 'modules/utility/gtb-helper' },
                { label: 'AntiDebuff', slug: 'modules/utility/antidebuff' },
              ],
            },
          ],
        },
        {
          label: 'Integration',
          items: [
            { label: 'Discord Rich Presence', slug: 'integration/discord-rpc' },
            { label: 'GUI Customization', slug: 'integration/gui-customization' },
          ],
        },
      ],
    }),
  ],
});
