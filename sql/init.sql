-- SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
-- SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
-- SPDX-FileCopyrightText: Copyright Contributors to the Exchange8:Refdata
-- SPDX-License-Identifier: Apache-2.0

-- Ex8:RefData - reference data: customers, markets, securities, tick rules,
-- calendars, books, corporate actions, identifiers.
--
-- Design: design/sentin8-exchange8-data-model-technical-design.md §3.1.
-- No migration code (new platform): `sentin8-cli vertical init` applies this.

CREATE SCHEMA IF NOT EXISTS refdata;

-- Customer/firm master (legacy CUSTOMER).
CREATE TABLE IF NOT EXISTS refdata.customer (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id          UUID NOT NULL,
    company_name    TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'E' CHECK (status IN ('E','D','H')),
    is_vendor       BOOLEAN NOT NULL DEFAULT FALSE,
    stp_code        TEXT,
    stp_action      TEXT NOT NULL DEFAULT 'D'
                    CHECK (stp_action IN ('K','P','B','D','N')),
    exec_broker_id  TEXT UNIQUE,
    cuid            TEXT,                -- 5-char clearing id
    sub_participant TEXT,
    entitlement     BIGINT NOT NULL DEFAULT 0,
    book_tokens     TEXT[] NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_exchange8_customer_env ON refdata.customer (env_id);

-- Customer sites (legacy LOCATION).
CREATE TABLE IF NOT EXISTS refdata.customer_location (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id          UUID NOT NULL,
    customer_id     UUID NOT NULL REFERENCES refdata.customer(id),
    location_type   TEXT NOT NULL,       -- primary/shipping/billing
    address1        TEXT,
    address2        TEXT,
    city            TEXT,
    province        TEXT,
    postal_code     TEXT,
    country         TEXT,
    status          TEXT NOT NULL DEFAULT 'E' CHECK (status IN ('E','D'))
);
CREATE INDEX IF NOT EXISTS idx_exchange8_custloc_env ON refdata.customer_location (env_id);

-- Customer contacts + contact↔location mapping.
CREATE TABLE IF NOT EXISTS refdata.customer_contact (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id      UUID NOT NULL,
    customer_id UUID NOT NULL REFERENCES refdata.customer(id),
    contact_type TEXT,
    name        TEXT NOT NULL,
    phone       TEXT,
    email       TEXT
);
CREATE TABLE IF NOT EXISTS refdata.customer_contact_location (
    env_id      UUID NOT NULL,
    contact_id  UUID NOT NULL REFERENCES refdata.customer_contact(id),
    location_id UUID NOT NULL REFERENCES refdata.customer_location(id),
    PRIMARY KEY (contact_id, location_id)
);

-- Traders (legacy BROKER_TRADER).
CREATE TABLE IF NOT EXISTS refdata.broker_trader (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id      UUID NOT NULL,
    user_id     TEXT NOT NULL,
    customer_id UUID NOT NULL REFERENCES refdata.customer(id),
    status      TEXT NOT NULL DEFAULT 'E' CHECK (status IN ('E','D')),
    email       TEXT,
    cuid_override TEXT,
    UNIQUE (env_id, user_id, customer_id)
);
CREATE INDEX IF NOT EXISTS idx_exchange8_btrader_env ON refdata.broker_trader (env_id);

-- Notional base per location+currency (legacy CAPITALIZATION).
CREATE TABLE IF NOT EXISTS refdata.capitalization (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id      UUID NOT NULL,
    customer_id UUID NOT NULL REFERENCES refdata.customer(id),
    location_id UUID NOT NULL REFERENCES refdata.customer_location(id),
    currency    TEXT NOT NULL,
    amount      NUMERIC(20,2) NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS refdata.currency (
    code            TEXT PRIMARY KEY,    -- 3-letter ISO
    env_id          UUID NOT NULL,
    name            TEXT,
    lis_exchange_rate NUMERIC(18,8) NOT NULL DEFAULT 1.0
);

-- Market definitions (legacy MARKET). market_mode D14.
CREATE TABLE IF NOT EXISTS refdata.market (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id          UUID NOT NULL,
    code            CHAR(1) NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    mic             TEXT,
    market_code     TEXT,                -- book grouping
    market_type     TEXT NOT NULL DEFAULT 'Exchange'
                    CHECK (market_type IN ('Exchange','MTF','Dark')),
    lot_size        INTEGER NOT NULL DEFAULT 100,
    hidden_orders   BOOLEAN NOT NULL DEFAULT FALSE,
    ebbo_enabled    BOOLEAN NOT NULL DEFAULT FALSE,
    regulation      TEXT,
    market_mode     TEXT NOT NULL DEFAULT 'calendar'
                    CHECK (market_mode IN ('calendar','continuous')),
    UNIQUE (env_id, code)
);

-- MIC registry (legacy VENUE).
CREATE TABLE IF NOT EXISTS refdata.venue (
    mic     TEXT NOT NULL,
    env_id  UUID NOT NULL,
    name    TEXT NOT NULL,
    PRIMARY KEY (env_id, mic)
);

-- Security master (legacy SECURITY). price_scale/qty_scale D13.
CREATE TABLE IF NOT EXISTS refdata.security (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id              UUID NOT NULL,
    symbol              TEXT NOT NULL,
    isin                TEXT,
    security_type       TEXT NOT NULL DEFAULT 'Equity'
                        CHECK (security_type IN
                        ('Equity','Bond','Fund','Warrant','Future','Option','Crypto','Fx')),
    currency            TEXT NOT NULL REFERENCES refdata.currency(code),
    halted              BOOLEAN NOT NULL DEFAULT FALSE,
    trading_state       TEXT NOT NULL DEFAULT 'C' CHECK (trading_state IN ('O','C')),
    shortable           TEXT NOT NULL DEFAULT 'N' CHECK (shortable IN ('Y','E','N')),
    permanently_delisted BOOLEAN NOT NULL DEFAULT FALSE,
    market_id           UUID REFERENCES refdata.market(id),
    tick_rule_id        UUID,
    lis_class_id        UUID,
    odd_lot             BOOLEAN NOT NULL DEFAULT FALSE,
    settlement_term     TEXT,
    charge_tier         TEXT,
    min_size            INTEGER NOT NULL DEFAULT 1,
    dividend_ind        BOOLEAN NOT NULL DEFAULT FALSE,
    contract_id         UUID,            -- futures/options extension (MARGIN-owned)
    asset_id            UUID,            -- crypto extension (ST-owned)
    price_scale         SMALLINT NOT NULL DEFAULT 4,   -- D13: equity ×10⁴
    qty_scale           SMALLINT NOT NULL DEFAULT 0,   -- D13: crypto ×10⁸
    UNIQUE (env_id, symbol)
);
CREATE INDEX IF NOT EXISTS idx_exchange8_security_env ON refdata.security (env_id);

-- Fixed-income extension (legacy FIXED_INCOME).
CREATE TABLE IF NOT EXISTS refdata.fixed_income (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id      UUID NOT NULL,
    security_id UUID NOT NULL REFERENCES refdata.security(id),
    frequency   INTEGER,
    coupon      NUMERIC(10,6),
    yield_pct  NUMERIC(10,6),
    issue_date  DATE,
    maturity_date DATE,
    par_price   NUMERIC(18,6),
    eom         BOOLEAN NOT NULL DEFAULT FALSE,
    term        TEXT
);

-- Large-in-scale thresholds (legacy LARGE_IN_SCALE).
CREATE TABLE IF NOT EXISTS refdata.large_in_scale (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id      UUID NOT NULL,
    class_id    TEXT NOT NULL,
    order_volume BIGINT NOT NULL
);

-- Tick bands (legacy TICK_SIZE_RULE / TICK_SIZE_VALUE).
CREATE TABLE IF NOT EXISTS refdata.tick_rule (
    id      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id  UUID NOT NULL,
    name    TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS refdata.tick_rule_value (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id      UUID NOT NULL,
    rule_id     UUID NOT NULL REFERENCES refdata.tick_rule(id),
    min_price   NUMERIC(18,4) NOT NULL,
    max_price   NUMERIC(18,4),
    tick_value  NUMERIC(18,4) NOT NULL
);

-- Market calendar (legacy HOLIDAY / HOLIDAY_MARKET).
CREATE TABLE IF NOT EXISTS refdata.holiday (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id      UUID NOT NULL,
    holiday_date DATE NOT NULL,
    name        TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS refdata.holiday_market (
    env_id      UUID NOT NULL,
    holiday_id  UUID NOT NULL REFERENCES refdata.holiday(id),
    market_id   UUID NOT NULL REFERENCES refdata.market(id),
    PRIMARY KEY (holiday_id, market_id)
);;

-- Order books (legacy MEGAMP_BOOK).
CREATE TABLE IF NOT EXISTS refdata.book (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id      UUID NOT NULL,
    code        TEXT NOT NULL UNIQUE,    -- 4-char book code
    token       CHAR(1) NOT NULL UNIQUE,
    default_flag BOOLEAN NOT NULL DEFAULT FALSE,
    preferencing TEXT NOT NULL DEFAULT 'TIME'
                CHECK (preferencing IN ('TIME','BROKER')),
    anonymous   BOOLEAN NOT NULL DEFAULT FALSE
);

-- Book↔connection membership (legacy MEGAMP_BOOK_CONNECTION).
CREATE TABLE IF NOT EXISTS refdata.book_connection (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id      UUID NOT NULL,
    book_id     UUID NOT NULL REFERENCES refdata.book(id),
    connection_id TEXT NOT NULL,         -- GW-owned connection comp id
    seq         INTEGER NOT NULL DEFAULT 1,
    status      TEXT NOT NULL DEFAULT 'E' CHECK (status IN ('E','D'))
);

-- Session times per book (legacy MEGAMP_BOOK_EVENT).
CREATE TABLE IF NOT EXISTS refdata.book_event (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id      UUID NOT NULL,
    book_id     UUID NOT NULL REFERENCES refdata.book(id),
    event_code  CHAR(1) NOT NULL,        -- SystemEventCode
    event_time  TIME NOT NULL
);

-- Security identifier cross-references.
CREATE TABLE IF NOT EXISTS refdata.security_identifier (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id      UUID NOT NULL,
    security_id UUID NOT NULL REFERENCES refdata.security(id),
    id_type     TEXT NOT NULL CHECK (id_type IN ('SYMBOL','ISIN','CUSIP','SEDOL','FIGI')),
    id_value    TEXT NOT NULL,
    UNIQUE (env_id, id_type, id_value)
);

-- Corporate-action master.
CREATE TABLE IF NOT EXISTS refdata.corporate_action (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id          UUID NOT NULL,
    security_id     UUID NOT NULL REFERENCES refdata.security(id),
    action_type     TEXT NOT NULL CHECK (action_type IN
                    ('cash_dividend','stock_dividend','split','reverse_split',
                     'merger','spin_off','rights','symbol_change','delisting')),
    announce_date   DATE,
    ex_date         DATE,
    record_date     DATE,
    pay_date        DATE,
    numerator       NUMERIC(18,6),
    denominator     NUMERIC(18,6),
    stage           TEXT NOT NULL DEFAULT 'announced' CHECK (stage IN
                    ('announced','ex_date','record_date','pay_date'))
);
CREATE INDEX IF NOT EXISTS idx_exchange8_ca_env ON refdata.corporate_action (env_id);

-- Record-date entitlement snapshot.
CREATE TABLE IF NOT EXISTS refdata.corporate_action_entitlement (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id      UUID NOT NULL,
    action_id   UUID NOT NULL REFERENCES refdata.corporate_action(id),
    holder_id   UUID NOT NULL,
    qty         NUMERIC(18,4) NOT NULL
);

-- Upcoming actions per env.
CREATE TABLE IF NOT EXISTS refdata.corporate_action_calendar (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id      UUID NOT NULL,
    action_id   UUID NOT NULL REFERENCES refdata.corporate_action(id),
    effective_date DATE NOT NULL
);

-- ---------------------------------------------------------------------------
-- Seed (legacy parity base config)
-- ---------------------------------------------------------------------------

INSERT INTO refdata.currency (code, env_id, name, lis_exchange_rate) VALUES
    ('CAD', '00000000-0000-0000-0000-000000000000', 'Canadian Dollar', 1.0),
    ('USD', '00000000-0000-0000-0000-000000000000', 'US Dollar', 1.35),
    ('BTC', '00000000-0000-0000-0000-000000000000', 'Bitcoin', 95000.0)
ON CONFLICT DO NOTHING;

INSERT INTO refdata.venue (mic, env_id, name) VALUES
    ('XTSX', '00000000-0000-0000-0000-000000000000', 'TSX Venture'),
    ('XTSE', '00000000-0000-0000-0000-000000000000', 'Toronto Stock Exchange'),
    ('XCNQ', '00000000-0000-0000-0000-000000000000', 'CNSX / Chi-X Canada'),
    ('LX',   '00000000-0000-0000-0000-000000000000', 'LX (dark)')
ON CONFLICT DO NOTHING;

-- ---------------------------------------------------------------------------
-- RLS (architecture-principles.md §4.3): enable + force on all tenant tables.
-- Policies use the platform helpers sentin8_context_initialized() and
-- sentin8_current_env_id(); services call set_tenant_context on the same
-- connection as the data query.
-- ---------------------------------------------------------------------------
DO $$
DECLARE tbl TEXT;
BEGIN
    FOREACH tbl IN ARRAY ARRAY['refdata.customer', 'refdata.customer_location', 'refdata.customer_contact', 'refdata.customer_contact_location', 'refdata.broker_trader', 'refdata.capitalization', 'refdata.currency', 'refdata.market', 'refdata.venue', 'refdata.security', 'refdata.fixed_income', 'refdata.large_in_scale', 'refdata.tick_rule', 'refdata.tick_rule_value', 'refdata.holiday', 'refdata.holiday_market', 'refdata.book', 'refdata.book_connection', 'refdata.book_event', 'refdata.security_identifier', 'refdata.corporate_action', 'refdata.corporate_action_entitlement', 'refdata.corporate_action_calendar'] LOOP
        BEGIN
            EXECUTE format('ALTER TABLE %s ENABLE ROW LEVEL SECURITY', tbl);
            EXECUTE format('ALTER TABLE %s FORCE ROW LEVEL SECURITY', tbl);
            EXECUTE format('DROP POLICY IF EXISTS %1$I_select ON %2$s;
CREATE POLICY %1$I_select ON %2$s FOR SELECT USING (sentin8_context_initialized() AND env_id = sentin8_current_env_id())', split_part(tbl, '.', 2), tbl);
            EXECUTE format('DROP POLICY IF EXISTS %1$I_insert ON %2$s;
CREATE POLICY %1$I_insert ON %2$s FOR INSERT WITH CHECK (sentin8_context_initialized() AND env_id = sentin8_current_env_id())', split_part(tbl, '.', 2), tbl);
            EXECUTE format('DROP POLICY IF EXISTS %1$I_update ON %2$s;
CREATE POLICY %1$I_update ON %2$s FOR UPDATE USING (sentin8_context_initialized() AND env_id = sentin8_current_env_id())', split_part(tbl, '.', 2), tbl);
            EXECUTE format('DROP POLICY IF EXISTS %1$I_delete ON %2$s;
CREATE POLICY %1$I_delete ON %2$s FOR DELETE USING (sentin8_context_initialized() AND env_id = sentin8_current_env_id())', split_part(tbl, '.', 2), tbl);
        EXCEPTION WHEN OTHERS THEN
            RAISE NOTICE 'RLS setup failed for %: %', tbl, SQLERRM;
        END;
    END LOOP;
END $$;

-- ---------------------------------------------------------------------------
-- Per-service role grants (schema-per-service: this service owns schema
-- `refdata`; grants live in db `exchange8` only - design doc §1).
-- ---------------------------------------------------------------------------
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'exchange8_refdata_service') THEN
        ALTER ROLE exchange8_refdata_service SET search_path = refdata, public;
        GRANT USAGE ON SCHEMA refdata TO exchange8_refdata_service;
        GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA refdata TO exchange8_refdata_service;
        GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA refdata TO exchange8_refdata_service;
    END IF;
END $$;

-- ---------------------------------------------------------------------------
-- Local outbox (DBX §6.7 - generated from `sentin8_outbox::outbox_table_sql(
-- "refdata")`): rows written in the same DB transaction as the domain change;
-- the shell's relay (boot step §8b) pushes them to Bus `POST /v1/produce`.
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS refdata.outbox (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_id          UUID NOT NULL,
    event_type      TEXT NOT NULL,
    stream_family   TEXT NOT NULL,
    payload         JSONB NOT NULL,
    publisher_id_token  TEXT,
    publisher_subject   TEXT,
    relayed         BOOLEAN NOT NULL DEFAULT false,
    relayed_at      TIMESTAMPTZ,
    relay_attempts  INT NOT NULL DEFAULT 0,
    next_retry_at   TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_refdata_outbox_unrelayed
    ON refdata.outbox (created_at) WHERE relayed = false;
DO $$ BEGIN
    EXECUTE 'ALTER TABLE refdata.outbox ENABLE ROW LEVEL SECURITY';
    EXECUTE 'ALTER TABLE refdata.outbox FORCE ROW LEVEL SECURITY';
    EXECUTE 'DROP POLICY IF EXISTS outbox_select ON refdata.outbox';
    EXECUTE 'CREATE POLICY outbox_select ON refdata.outbox FOR SELECT
        USING (sentin8_context_initialized()
               AND env_id = sentin8_current_env_id())';
    EXECUTE 'DROP POLICY IF EXISTS outbox_insert ON refdata.outbox';
    EXECUTE 'CREATE POLICY outbox_insert ON refdata.outbox FOR INSERT
        WITH CHECK (sentin8_context_initialized()
               AND env_id = sentin8_current_env_id())';
    EXECUTE 'DROP POLICY IF EXISTS outbox_update ON refdata.outbox';
    EXECUTE 'CREATE POLICY outbox_update ON refdata.outbox FOR UPDATE
        USING (sentin8_context_initialized()
               AND env_id = sentin8_current_env_id())';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'RLS setup failed for refdata.outbox: %', SQLERRM;
END $$;

CREATE OR REPLACE FUNCTION refdata.refdata_notify_outbox() RETURNS TRIGGER
LANGUAGE plpgsql AS $fn$
BEGIN
    PERFORM pg_notify('refdata_outbox_changed', '');
    RETURN NEW;
END;
$fn$;
DROP TRIGGER IF EXISTS outbox_notify_trigger ON refdata.outbox;
CREATE TRIGGER outbox_notify_trigger
    AFTER INSERT ON refdata.outbox
    FOR EACH ROW
    EXECUTE FUNCTION refdata.refdata_notify_outbox();

-- ---------------------------------------------------------------------------
-- Local audit journal (DBX §6.8 - generated from
-- `sentin8_audit::audit_journal_sql("refdata")`): rows written in the same
-- transaction as the domain change; the shell's pusher (boot step §8c) pushes
-- them to Sys `POST /v1/audit` (`sentin8:sys:audit:write`). A Sys outage
-- never drops audit.
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS refdata.audit_journal (
    id              UUID PRIMARY KEY,
    env_id          UUID NOT NULL,
    action          TEXT NOT NULL,
    target_resource_type TEXT,
    target_resource_id    TEXT,
    actor_user_id   TEXT,
    actor_subject_type TEXT,
    severity        TEXT NOT NULL DEFAULT 'info',
    correlation_id  TEXT,
    before_state    JSONB,
    after_state     JSONB,
    details         JSONB,
    pushed          BOOLEAN NOT NULL DEFAULT false,
    pushed_at       TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_refdata_audit_journal_unpushed
    ON refdata.audit_journal (created_at) WHERE pushed = false;
DO $$ BEGIN
    EXECUTE 'ALTER TABLE refdata.audit_journal ENABLE ROW LEVEL SECURITY';
    EXECUTE 'ALTER TABLE refdata.audit_journal FORCE ROW LEVEL SECURITY';
    EXECUTE 'DROP POLICY IF EXISTS audit_journal_dml ON refdata.audit_journal';
    EXECUTE 'CREATE POLICY audit_journal_dml ON refdata.audit_journal
        USING (sentin8_context_initialized()
               AND env_id = sentin8_current_env_id())
        WITH CHECK (sentin8_context_initialized()
               AND env_id = sentin8_current_env_id())';
EXCEPTION WHEN OTHERS THEN
    RAISE NOTICE 'RLS setup failed for refdata.audit_journal: %', SQLERRM;
END $$;

-- Outbox + journal role grants (same pattern as the table grants above).
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'exchange8_refdata_service') THEN
        GRANT SELECT, INSERT, UPDATE ON ALL TABLES IN SCHEMA refdata TO exchange8_refdata_service;
    END IF;
END $$;
