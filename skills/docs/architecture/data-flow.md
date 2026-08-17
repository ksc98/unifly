# Data Flow

## Connection Lifecycle

```mermaid
sequenceDiagram
    participant User
    participant Controller
    participant IntegrationAPI
    participant LegacyAPI
    participant WebSocket
    participant DataStore

    User->>Controller: connect()
    Controller->>IntegrationAPI: Authenticate (API key)
    Controller->>LegacyAPI: Authenticate (cookie + CSRF)
    Controller->>IntegrationAPI: Fetch all entities
    IntegrationAPI-->>DataStore: Store devices, clients, networks...
    Controller->>LegacyAPI: Fetch events, health
    LegacyAPI-->>DataStore: Store events, health summaries
    Controller->>WebSocket: Connect event stream
    Controller->>Controller: Spawn refresh task (30s)
    Controller->>Controller: Spawn command processor

    loop Every 30 seconds
        Controller->>IntegrationAPI: Refresh entities
        IntegrationAPI-->>DataStore: Update collections
        DataStore-->>User: Notify via watch channels
    end

    loop Real-time
        WebSocket-->>DataStore: Push events
        DataStore-->>User: Notify subscribers
    end
```

## Data Store Architecture

The `DataStore` uses a combination of `DashMap` and `tokio::watch` for lock-free reactive storage:

```mermaid
graph LR
    subgraph DataStore
        DM["DashMap&lt;String, Arc&lt;T&gt;&gt;<br/><i>Lock-free concurrent map</i>"]
        WS["watch::Sender&lt;Arc&lt;Vec&lt;T&gt;&gt;&gt;<br/><i>Change notification</i>"]
    end

    API["API Response"] --> DM
    DM --> WS
    WS --> CLI["CLI (read once)"]
    WS --> TUI["TUI (subscribe)"]
```

- **Writes** go through `DashMap::insert()` then `watch::Sender::send()`
- **CLI reads** call `current()` — snapshot of the latest data
- **TUI subscribes** via `changed()` — async notification on updates

## Entity ID Resolution

Entities can have different IDs depending on the API source:

| API | ID Format | Example |
|---|---|---|
| Integration | UUID v4 | `a1b2c3d4-e5f6-7890-abcd-ef1234567890` |
| Legacy | MAC address | `fc:ec:da:ab:cd:ef` |
| Synthetic | Prefixed string | `net:a1b2c3d4`, `wifi:e5f6a7b8` |

The `EntityId` enum handles this transparently:

```rust
enum EntityId {
    Uuid(Uuid),      // Integration API entities
    Legacy(String),  // Legacy API entities (MAC-based)
}
```

Non-MAC entities (networks, WiFi, firewall policies) use synthetic keys with a type prefix to avoid collisions in the shared `DashMap`.

## CLI vs TUI Data Patterns

| Pattern | CLI | TUI |
|---|---|---|
| **Connection** | `oneshot()` — no background tasks | `connect()` — full lifecycle |
| **Data access** | Single `current()` snapshot | `changed()` subscription loop |
| **Refresh** | None (fire-and-forget) | Automatic every 30 seconds |
| **Events** | Optional stream command | Always connected via WebSocket |
