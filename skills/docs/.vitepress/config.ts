import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'Unifly',
  description: 'CLI + TUI for UniFi Network Controllers',
  base: '/unifly/',

  head: [
    ['meta', { name: 'theme-color', content: '#e135ff' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:title', content: 'Unifly Documentation' }],
    ['meta', { property: 'og:description', content: 'CLI + TUI for UniFi Network Controllers' }],
  ],

  themeConfig: {
    nav: [
      { text: 'Guide', link: '/guide/' },
      { text: 'CLI', link: '/reference/cli' },
      { text: 'TUI', link: '/reference/tui' },
      { text: 'Architecture', link: '/architecture/' },
    ],

    sidebar: {
      '/guide/': [
        {
          text: 'Getting Started',
          items: [
            { text: 'Introduction', link: '/guide/' },
            { text: 'Installation', link: '/guide/installation' },
            { text: 'Quick Start', link: '/guide/quick-start' },
            { text: 'Configuration', link: '/guide/configuration' },
            { text: 'Authentication', link: '/guide/authentication' },
          ]
        }
      ],
      '/reference/': [
        {
          text: 'Reference',
          items: [
            { text: 'CLI Commands', link: '/reference/cli' },
            { text: 'TUI Dashboard', link: '/reference/tui' },
          ]
        }
      ],
      '/architecture/': [
        {
          text: 'Architecture',
          items: [
            { text: 'Overview', link: '/architecture/' },
            { text: 'Crate Structure', link: '/architecture/crates' },
            { text: 'Data Flow', link: '/architecture/data-flow' },
            { text: 'API Surface', link: '/architecture/api-surface' },
          ]
        }
      ],
    },

    socialLinks: [
      { icon: 'github', link: 'https://github.com/hyperb1iss/unifly' }
    ],

    footer: {
      message: 'Released under the Apache 2.0 License.',
      copyright: 'Copyright \u00a9 2025 Stefanie Jane'
    },

    search: {
      provider: 'local'
    }
  },

  markdown: {
    theme: {
      light: 'github-light',
      dark: 'one-dark-pro'
    }
  }
})
