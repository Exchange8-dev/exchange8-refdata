# Exchange8:RefData - Business Requirements & Functionality

> Audience: product owners, business analysts, and anyone evaluating what
> Exchange8:RefData **does for the business**. For architecture read `README.md`;
> for wire-level detail read `DESIGN.md`.

## 1. Business Purpose

- Be the single source of truth for the trading universe: what instruments
  exist, where they trade, in what currency, at what precision, and in what
  market state.
- Turn corporate-action announcements (dividends, splits, mergers, spin-offs,
  rights, symbol changes, delistings) into a controlled, auditable lifecycle
  with per-holder entitlements - so holders are credited exactly once and
  market data stays consistent through the transition.
- Give every tenant (environment) an isolated reference-data namespace:
  securities, customers, books and calendars are invisible across tenants by
  construction (Row-Level Security), not by convention.
- Protect order flow: read access to the security master is available to
  order-placing clients without giving them any write power over reference
  data.
- Serve as the identity cross-walk of the market - symbol, ISIN, CUSIP,
  SEDOL and FIGI all resolve to one security, with conflicting mappings
  flagged for operations rather than guessed.

## 2. Business Scope

### What the service provides

| Capability | Where it lives today | Notes |
|---|---|---|
| Security master with per-env isolation - list/detail/create/update/delete securities | Implemented - `GET/POST /v1/securities`, `GET/PUT/DELETE /v1/securities/{id}` (+ `/v1/admin/securities` aliases incl. POST `/update`/`/delete`) | filters by symbol, type, market; page limit capped; referenced rows delete-guarded (delist instead) |
| Precision contract (`price_scale`, `qty_scale`, `min_size`) | Implemented - security create + update with defaults (×10⁴ prices) | consumers key on the scales |
| Market definitions (Exchange/MTF/Dark, lot size, calendar vs continuous mode) | Implemented - `GET/POST /v1/markets`, `PUT /v1/markets/{id}` (+ `/v1/admin/markets`, POST `/update`) | single-char code unique per env; type/mode vocabulary validated |
| MIC venue registry + currency/FX-rate registry | Table-backed (seeded CAD/USD/BTC, XTSX/XTSE/XCNQ/LX) | `refdata.venue`, `refdata.currency` |
| Customer/firm directory with sites, contacts, traders, notional capitalization | Table-backed - API not yet exposed | `refdata.customer*`, `refdata.broker_trader`, `refdata.capitalization` |
| Order-book configuration (book codes, tokens, preferencing, session times, connections) | Membership API live - `POST /v1/books/connections` (outbox event); book master/session times table-backed | `refdata.book*` |
| Tick rules & tick bands | Implemented - `GET/POST /v1/tick-rules`, `PUT /v1/tick-rules/{id}` (rule + bands in one write) | `refdata.tick_rule*` |
| Market holiday calendars | Implemented - `POST /v1/holidays` (date + name + per-market closure rows) | reads table-backed; `refdata.holiday*` |
| Security identifier cross-reference (SYMBOL/ISIN/CUSIP/SEDOL/FIGI) with conflict flagging | Implemented as domain library + table | `CrossRef`, `refdata.security_identifier` |
| Corporate-action catalogue: 9 action kinds with ratio, dates and lifecycle stage | Implemented - announce (`POST /v1/corporate-actions`), cancel, calendar view; transitions are Sentin8:Cron-driven (the proxy-back executes them) | `refdata.corporate_action*` |
| Ex-date adjustment math (positions & reference prices) and entitlement snapshots | Implemented as domain library (value-preserving integer math) | `adjust_qty`, `adjust_price`, `snapshot_entitlements` |
| Corporate-action calendar windows (ex/record/pay per date) | Implemented as domain library | `CaCalendar` |
| Shield policy administration (policy sets, relations, attributes) | Implemented - shell-mounted, per-service scope | policy-admin API |
| Admin console pages for the reference-data surface (RFW) | Implemented - `GET /v1/admin/ui/{securities|markets|corporate-actions}` page definitions + `/v1/admin/{resource}` data endpoints (POST-alias mutations) | every mutation journals + emits |

### What the service does NOT own (business boundary)

| Concern | Owning service |
|---|---|
| Trade capture, trade ledger, trade busts | Exchange8:Clearing |
| Order matching and instrument halt/resume execution | Exchange8:ME |
| Delivery of cash/stock entitlements | Exchange8:Settlement |
| Margin and collateral treatment (futures/options) | Exchange8:Margin |
| Order-management state and client orders | Exchange8:OMS |
| Market-data publication and replay | Exchange8:MD |
| Venue feed adapters and external data ingest | Exchange8:VFeeds |
| Smart order routing | Exchange8:SOR |
| Surveillance detection and case actions | Exchange8:Surveillance |
| Regulatory reporting | Exchange8:Regulatory |
| FX pair quotes and RFQ negotiation | Exchange8:RFQ |
| Corporate-action stage transitions (scheduling) | Sentin8:Cron |
| Event transport and durable streams | Sentin8:Bus |
| Identity, tokens, roles | Sentin8:Auth |
| Audit-event ingest | Sentin8:Sys |
| Admin UI rendering | Sentin8:Dash |

## 3. Business Capabilities

### Security master

| Capability | Business rule |
|---|---|
| Create security | Requires a write-authorized principal; symbol must be non-empty; symbol is unique per environment (duplicate → conflict); currency must exist; precision defaults to ×10⁴ price scale, integer quantities |
| List securities | Any reader (refdata reader, order-placer, or env/platform admin) sees only its own environment's securities, filterable and capped |
| Get security | Same isolation; unknown id → not-found error code `exchange8_security_not_found` |
| Market state fields | `trading_state` Open/Closed, `halted` flag, `shortable` Y/E/N, `permanently_delisted` - the operating states every service keys trading behavior against |

### Customer & firm directory

| Capability | Business rule |
|---|---|
| Customer master | Each firm carries status (E enabled / D disabled / H held), vendor flag, STP code + STP action, a unique executing-broker id, a 5-char clearing id (CUID), and book tokens it may trade in |
| Sites (locations) | Primary/shipping/billing addresses per firm, enablement status per site |
| Contacts & mapping | Contact rows (type, name, phone, email) tied to firms and to their locations |
| Traders | A trader belongs to exactly one firm per environment (`user_id` unique per firm), with optional CUID override |
| Capitalization | Notional base per (location, currency) - the notional base business relies on for entitlement/capitalization checks |

### Markets, venues & calendars

| Capability | Business rule |
|---|---|
| Market definitions | One-character market code unique per environment; type Exchange/MTF/Dark; lot size; hidden-order and EBBO flags; calendar vs continuous market mode |
| MIC venues | ISO 10383 MIC registry with names, per environment |
| Currencies | 3-letter ISO code registry with the LIS exchange rate used by LIS checks and FX-adjacent flows |
| Holidays | Market holiday calendar (date + name) mapped per market - feeding trading-calendar behavior |

### Order books

| Capability | Business rule |
|---|---|
| Book master | 4-character book code and single-character token both unique; time vs broker preferencing; anonymous flag; default book flag |
| Book membership | Books carry connections (gateway comp ids) with sequence and enablement status |
| Session times | Per-book event codes and times define the trading session |

### Tick rules & LIS

| Capability | Business rule |
|---|---|
| Tick rule values | Price-band table (min/max/tick) applied per security through `refdata.security.tick_rule_id` |
| Large-in-scale | Class thresholds (`order_volume`) for LIS classification |

### Corporate actions

| Capability | Business rule |
|---|---|
| Action catalogue | Nine kinds: cash dividend, stock dividend, split, reverse split, merger, spin-off, rights, symbol change, delisting |
| Lifecycle | announced → ex-date → record-date → pay-date; pay-date is terminal; irreversible kinds (merger, delisting) terminate at ex-date with removal rather than record/pay |
| Quantity adjustments | Split multiplies quantity by the ratio; reverse split consolidates; stock dividend adds the ratio; cash actions leave quantity unchanged |
| Price adjustments | Scales/divides for splits and stock dividends; subtracts cash per share on the ex-date for dividends |
| Entitlements | Record-date snapshot: cash per share × holding for dividends; new shares per held share for stock actions, spin-offs and rights |
| Scheduling | Transitions run on Sentin8:Cron; Exchange8:Settlement performs delivery on the pay date |

### Cross-reference resolution

| Capability | Business rule |
|---|---|
| Resolve by scheme+value | Exact lookup in SYMBOL, ISIN, CUSIP, SEDOL, FIGI |
| Resolve any scheme | All scheme matches must agree on one security; disagreement is flagged for operations, never auto-resolved |
| Reverse view | All identifiers of one security, scheme-sorted |

## 4. Business Events (Outbound)

| Event (vocabulary) | When it fires | Status |
|---|---|---|
| `exchange8:refdata:security:created` / `…:updated` / `…:deleted` | A security row is created/updated/deleted | Emitted (outbox row in the domain transaction; relay pushes to Bus) |
| `exchange8:refdata:market:created` / `…:updated` | A market row is created/updated | Emitted |
| `exchange8:refdata:corporate-action:announced` | An action is announced (stage `announced`) | Emitted |
| `exchange8:refdata:corporate-action:stage-advanced` / `…:completed` | The Sentin8:Cron transition run advances a stage; `completed` = the terminal pay-date handoff to Exchange8:Settlement | Emitted per advanced transition |
| `exchange8:refdata:corporate-action:cancelled` | An operator withdraws an action still in `announced` | Emitted |
| `exchange8:refdata:tick-rule:created` / `…:updated` | Tick rule + bands written | Emitted |
| `exchange8:refdata:book-connection:created` | A book↔connection membership is registered | Emitted |
| `exchange8:refdata:holiday:created` | A market holiday is declared | Emitted |
| Audit events to Sentin8:Sys (`POST /v1/audit`) | Every mutation (the journal row is written in the same domain transaction) | Emitted |

**Inbound (consumed):** none today. The transition work ARRIVES as the
Sentin8:Cron proxy-back (`POST /v1/internal/schedules/run`, M2M) rather
than a bus event; Shield policy-set changes reach it through the shell's
hot-reload consumer rather than business events. Position snapshots for
record-date entitlements would be the future inbound surface.

## 5. Business Rules & Invariants

1. Each environment sees exactly its own reference data - enforced by RLS on
   every table, plus the explicit environment filter on every query; a
   caller with no tenant context sees nothing and may write nothing.
2. Securities are unique by symbol per environment; currency must resolve to
   the currency registry.
3. Reference data is read-only for order-placing clients; only refdata
   writers (and platform administrators) may create or change securities.
4. Every corporate action runs its lifecycle in order - announcements precede
   ex-dates, record dates precede pay dates; pay-date is terminal and no
   stage can be skipped or reversed.
5. Ex-date adjustments are value-preserving: $qty × price$ (in raw ticks) is
   invariant across split/reverse-split/cash-dividend adjustments (integer
   raw-tick math, no floating point, no rounding drift).
6. Entitlements are computed from the record-date position snapshot only -
   holders at record date, not at pay date, receive the entitlement.
7. Identifier cross-references never guess: a value mapping to two
   securities through different schemes is flagged, not resolved.
8. Reference data contains no secrets: provider credentials, keys, or payment
   details never appear in any `refdata` table.
9. Events are delivered at-least-once with dedupe by the outbox row id -
   consumers can treat delivery as effectively exactly-once.
10. Audit is never lost to an outage: journal rows persist locally until
    Sentin8:Sys acknowledges them.

## 6. Target-User Roles & Use Cases

| Persona | Role/scope | What they do with Exchange8:RefData |
|---|---|---|
| Exchange operator (env admin) | `exchange8:admin`, refdata write | Creates and maintains securities, markets, books, calendars; declares corporate actions |
| Platform operations | `sentin8:sys:admin` | Cross-env administration, policy administration, health and upgrade supervision |
| Order-placement client (member firm) | `exchange8:order:read` | Reads the securities it trades against (precision, trading state, shortability) |
| Compliance/issuer ops | `exchange8:corporate-action:read|write` | Announces dividends/splits, reviews the corporate-action calendar and entitlement snapshots |
| Matching/venue services | M2M with declared SELECT grants | Reads `refdata.security` for instrument behavior at the trading hot path |
| Market-data/venue-feed services | M2M with declared SELECT grants | Re-keys feeds against the security master and identifiers |
| Business analyst | Dashboard consumer | Views securities, markets and corporate-action pages through Sentin8:Dash RFW |

## 7. KPIs / Success Criteria

- **Correctness**: zero value drift across corporate-action adjustments
  (quantity × price preserved in raw ticks); entitlement snapshots match
  position records at record date.
- **Isolation**: zero cross-environment reference-data leakage (the SQL
  matrix - no context, own env, cross env - passes on every table).
- **Read latency**: security lookups answered from a single indexed query on
  `env_id` (no N+1 behavior, per-request limit ≤ 500).
- **Availability**: the service boots and serves read traffic even when
  Sentin8:Bus or Sys is down (fail-open durability tasks keep retries
  local); liveness endpoint exposes in-flight count for zero-downtime
  upgrades.
- **Security posture**: no BYPASSRLS role, no plaintext secrets, all
  authorization decided by Shield with the registered mirror set covering
  every endpoint gate (wiring tests prove coverage parity).
- **Throughput ceiling by design**: low event volume (dozens of corporate
  actions per day) - correctness outranks throughput.

## 8. Dependencies & Reliance on Other Services

| Reliance | Why |
|---|---|
| Sentin8:Auth | Only identity issuer; JWKS verification, M2M client registration (DCR, cert-bound) for all five workloads |
| Sentin8:Policy (Shield) | The step-5 authorization decision over the live effective scope set; policy sets and hot reload |
| Sentin8:DB (PostgreSQL 18, db `exchange8`) | All persistence; RLS helpers; per-service role |
| Sentin8:Bus | Event transport for the outbox relay (produce API, consumer groups, DLQ) |
| Sentin8:Sys | Audit ingest (`POST /v1/audit`), hostname resolution for the database-free TLS path |
| Sentin8:Cron | Scheduling of corporate-action lifecycle transitions and calendar-driven work |
| Sentin8:Shell (sentin8 crate) | Boot order, capped pool, health, middleware, policy-admin mount, relays, TLS library |
| Sentin8:Dash | Renders the RFW admin pages; Sentin8:BFF proxies all client traffic |
| Exchange8:Settlement | Delivers cash/stock entitlements on the pay date |
| Exchange8:Clearing | Source of position snapshots for entitlement modeling; position adjustments applied there |

## 9. Acceptance Criteria

1. A tenant administrator can list securities for its environment, filtered
   by symbol/type/market, and create a security with unique symbol,
   existing currency, and precise scales - receiving a full projection
   response.
2. A client holding only the order-reading scope can read securities but
   cannot create or change them (403 on write paths).
3. A caller from another environment (or with no tenant context) sees zero
   rows and cannot insert - verified per table against the shipped schema.
4. Every endpoint rejects bearer-less or wrongly-scoped calls with 401/403
   and never touches the database on auth failure; errors are plain text and
   never leak SQL internals.
5. Corporate-action math passes its property-style tests: split 2:1,
   reverse split 1:10, cash and stock dividends compute quantity, price and
   entitlement values exactly.
6. The RFW admin surface serves icon-hex, POST-alias-friendly,
   scope-stamped page definitions (`securities` / `markets` /
   `corporate-actions`) that Sentin8:Dash can render; the matching data
   endpoints enforce the same decisions as the public routes.
7. The policy set declared at registration covers every gate the handlers
   evaluate (including the Cron proxy-back transition action) and passes
   the enrollment dry-run; tenant-authored forbid rules override platform
   permits.
8. Durability pairs behave to spec: every mutation writes the outbox
   envelope (payload-complete snapshot) + the audit journal row in the
   same transaction; outbox rows are produced with idempotency by row id
   and survive Bus outages; audit journal rows reach Sys and survive Sys
   outages.
9. The Sentin8:Cron transition run advances only rows whose milestone date
   has passed, dedupes on (action id, target stage), and re-runs safely
   after a missed tick (retroactive advancement, each step audited).
10. The service fails fast at boot against a wrong database or PostgreSQL
   below the required version, and upgrades drain in-flight requests via
   the health-payload connection count.

## 10. Open Business Questions

- The customer/location/contact/trader/capitalization master is
  table-complete but unexposed - which flows need those APIs first
  (onboarding, entitlements, capitalization checks) is a product call.
- Who is the authoritative source for corporate-action announcements -
  manual admin entry, an external feed via Exchange8:VFeeds, or both - and with
  what review/approval workflow before actions become effective.
- Reconciliation ownership for entitlement snapshots: Exchange8:RefData produces
  the ratio math and the record-date transition events; who owns the
  discrepancy report when snapshots diverge from the position ledger.
- Tenant self-service: may environment admins author their own currencies,
  markets and books, or are those platform-curated catalogs per vertical.
- Crypto corporate actions (forks, airdrops, staking rewards) are designed
  at the schema level (`security_type='Crypto'`, `asset_id`) with no
  action-kind support yet - product timing undecided.
- Hot-path caching of the security master for matching engines: acceptable
  staleness window and invalidation surface (MemBus state refresh vs
  event-driven re-key) is undecided.

---

**Architecture & Design:** exchange8:refdata and its underlying concepts were designed by [Hamid Jahanzad](https://www.linkedin.com/in/hamidjahanzad/).
