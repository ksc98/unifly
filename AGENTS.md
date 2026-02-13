# unifly-skill

UniFi network management skill for AI agents.

## Skills

- **unifly** — Complete UniFi infrastructure management via the unifly CLI. Covers devices, clients, networks, WiFi, firewall, DNS, VPN, monitoring, and automation.

## Commands

- **/unifly-status** — Quick health check of UniFi infrastructure
- **/unifly-audit** — Security audit of UniFi configuration

## Agents

- **unifly-network-manager** — Autonomous UniFi network management agent for devices, networks, WiFi, firewall, monitoring, and diagnostics

## Installation

```bash
# Claude Code
claude /plugin install hyperb1iss/unifly-skill

# Skills.sh
npx add-skill hyperb1iss/unifly-skill

# Manual
git clone https://github.com/hyperb1iss/unifly-skill.git ~/.claude/plugins/unifly-skill
```

## Requirements

- [unifly](https://github.com/hyperb1iss/unifly) CLI installed and configured
- Access to a UniFi controller (UDM, UCG, or self-hosted)
