// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// heal is hosted at https://kechol.github.io/heal/. The `base` matches the
// repository name so all links resolve under that prefix.
export default defineConfig({
  site: 'https://kechol.github.io',
  base: '/heal',
  integrations: [
    starlight({
      title: 'heal',
      description:
        'A code-health harness that measures your codebase on every commit and surfaces relevant changes to your AI agent.',
      logo: { src: './src/assets/logo.svg', replacesTitle: false },
      favicon: '/favicon.svg',
      customCss: ['./src/styles/custom.css'],
      // Drop the macOS-style terminal header (three traffic-light dots)
      // that Expressive Code adds to sh / bash code blocks by default.
      // `frame: 'code'` keeps the rounded code box but removes the
      // window chrome.
      expressiveCode: {
        defaultProps: { frame: 'code' },
      },
      defaultLocale: 'root',
      locales: {
        root: { label: 'English', lang: 'en' },
        ja: { label: '日本語', lang: 'ja' },
      },
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/kechol/heal',
        },
      ],
      editLink: {
        baseUrl: 'https://github.com/kechol/heal/edit/main/docs/',
      },
      sidebar: [
        {
          label: 'Start Here',
          translations: { ja: 'はじめに' },
          items: [
            {
              label: 'Quick Start',
              translations: { ja: 'クイックスタート' },
              slug: 'quick-start',
            },
            {
              label: 'Concept',
              translations: { ja: '設計思想' },
              slug: 'concept',
            },
            {
              label: 'Installation',
              translations: { ja: 'インストール' },
              slug: 'installation',
            },
          ],
        },
        {
          label: 'Reference',
          translations: { ja: 'リファレンス' },
          items: [
            {
              label: 'CLI',
              translations: { ja: 'CLI' },
              slug: 'cli',
            },
            {
              label: 'Configuration',
              translations: { ja: '設定' },
              slug: 'configuration',
            },
            {
              label: 'Metrics',
              translations: { ja: 'メトリクス' },
              slug: 'metrics',
            },
            {
              label: 'Claude plugin',
              translations: { ja: 'Claude プラグイン' },
              slug: 'claude-plugin',
            },
            {
              label: 'Architecture',
              translations: { ja: 'アーキテクチャ' },
              slug: 'architecture',
            },
          ],
        },
      ],
    }),
  ],
});
