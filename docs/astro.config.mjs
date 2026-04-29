// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// HEAL is hosted at https://kechol.github.io/heal/. The `base` matches the
// repository name so all links resolve under that prefix.
export default defineConfig({
  site: 'https://kechol.github.io',
  base: '/heal',
  integrations: [
    starlight({
      title: 'HEAL',
      description:
        'Hook-driven Evaluation & Autonomous Loop — a code-health harness that turns codebase decay signals into work for AI coding agents.',
      logo: { src: './src/assets/logo.svg', replacesTitle: false },
      favicon: '/favicon.svg',
      defaultLocale: 'root',
      locales: {
        root: { label: 'English', lang: 'en' },
        ja: { label: '日本語', lang: 'ja' },
      },
      social: {
        github: 'https://github.com/kechol/heal',
      },
      editLink: {
        baseUrl: 'https://github.com/kechol/heal/edit/main/docs/',
      },
      sidebar: [
        {
          label: 'Start Here',
          translations: { ja: 'はじめに' },
          items: [
            {
              label: 'Getting Started',
              translations: { ja: 'クイックスタート' },
              slug: 'getting-started',
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
