---
layout: home

hero:
  name: Unifly
  text: Your UniFi Network, at Your Fingertips
  tagline: CLI + TUI for UniFi Network Controllers — real-time monitoring, device management, and network administration from your terminal
  actions:
    - theme: brand
      text: Get Started
      link: /guide/
    - theme: alt
      text: View on GitHub
      link: https://github.com/hyperb1iss/unifly

features:
  - icon: "\u26A1"
    title: Dual API Engine
    details: Integration API (REST, API key) + Legacy API (session, cookie/CSRF) with automatic negotiation
  - icon: "\uD83D\uDCCA"
    title: Real-Time TUI
    details: btop-inspired dashboard with Braille traffic charts, CPU/MEM bars, and live client counts
  - icon: "\uD83D\uDD0D"
    title: 20+ Resource Types
    details: Devices, clients, networks, WiFi, firewall, DNS, VPN, hotspot vouchers, DPI, and more
  - icon: "\uD83D\uDD12"
    title: Secure Credentials
    details: OS keyring storage for API keys and passwords — nothing written to disk in plaintext
  - icon: "\uD83C\uDF10"
    title: Multi-Profile
    details: Named profiles for multiple controllers with instant switching via a single flag
  - icon: "\uD83D\uDCE1"
    title: WebSocket Events
    details: Live event streaming with severity filtering and real-time push notifications
---

<style>
:root {
  --vp-home-hero-name-color: transparent;
  --vp-home-hero-name-background: linear-gradient(135deg, #e135ff 0%, #80ffea 100%);
}

.dark {
  --vp-home-hero-image-background-image: linear-gradient(135deg, rgba(225, 53, 255, 0.2) 0%, rgba(128, 255, 234, 0.2) 100%);
  --vp-home-hero-image-filter: blur(56px);
}
</style>
