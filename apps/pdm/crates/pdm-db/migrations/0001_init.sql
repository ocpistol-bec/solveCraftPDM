-- PDM initial schema. Applied by pdm-db::migrate and tracked in schema_migrations.
-- Postgres 13+. UUID primary keys; org_id on every tenant-scoped table.
--
-- Phase 1 simplification: RBAC uses users.role ('viewer' | 'editor' | 'admin').
-- The roles/permissions/groups tables below exist for the full model (toDo §5)
-- and will be wired up in a later phase.

CREATE TABLE IF NOT EXISTS schema_migrations (
    version    INTEGER PRIMARY KEY,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS organizations (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS users (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id        UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    email         TEXT NOT NULL,
    display_name  TEXT NOT NULL DEFAULT '',
    password_hash TEXT,
    role          TEXT NOT NULL DEFAULT 'viewer' CHECK (role IN ('viewer', 'editor', 'admin')),
    active        BOOLEAN NOT NULL DEFAULT TRUE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, email)
);

CREATE TABLE IF NOT EXISTS groups (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id     UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    UNIQUE (org_id, name)
);

CREATE TABLE IF NOT EXISTS roles (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id     UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    UNIQUE (org_id, name)
);

CREATE TABLE IF NOT EXISTS permissions (
    id      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    role_id UUID NOT NULL REFERENCES roles(id) ON DELETE CASCADE,
    code    TEXT NOT NULL,
    UNIQUE (role_id, code)
);

CREATE TABLE IF NOT EXISTS memberships (
    id      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    group_id UUID REFERENCES groups(id) ON DELETE CASCADE,
    role_id UUID REFERENCES roles(id) ON DELETE CASCADE,
    UNIQUE (user_id, group_id, role_id)
);

CREATE TABLE IF NOT EXISTS service_accounts (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id     UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, name)
);

CREATE TABLE IF NOT EXISTS api_tokens (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id      UUID REFERENCES users(id) ON DELETE CASCADE,
    service_account_id UUID REFERENCES service_accounts(id) ON DELETE CASCADE,
    name         TEXT NOT NULL DEFAULT '',
    token_hash   TEXT NOT NULL UNIQUE,
    scopes       TEXT NOT NULL DEFAULT 'full',
    last_used_at TIMESTAMPTZ,
    expires_at   TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS projects (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id      UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, name)
);

CREATE TABLE IF NOT EXISTS folders (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id     UUID NOT NULL,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    parent_id  UUID REFERENCES folders(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS items (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id          UUID NOT NULL,
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    folder_id       UUID REFERENCES folders(id) ON DELETE CASCADE,
    part_number     TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    classification  TEXT NOT NULL DEFAULT 'GEN',
    doc_id          UUID,
    lifecycle_state TEXT NOT NULL DEFAULT 'wip',
    owner_id        UUID REFERENCES users(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, part_number)
);

CREATE TABLE IF NOT EXISTS number_sequences (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id     UUID NOT NULL,
    prefix     TEXT NOT NULL,
    next_value BIGINT NOT NULL DEFAULT 1,
    UNIQUE (org_id, prefix)
);

CREATE TABLE IF NOT EXISTS files (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    item_id    UUID NOT NULL UNIQUE REFERENCES items(id) ON DELETE CASCADE,
    filename   TEXT NOT NULL DEFAULT '',
    mime_type  TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS file_blobs (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id      UUID NOT NULL,
    sha256      TEXT NOT NULL,
    size_bytes  BIGINT NOT NULL DEFAULT 0,
    storage_key TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, sha256)
);

CREATE TABLE IF NOT EXISTS file_versions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    file_id         UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    seq             INTEGER NOT NULL,
    blob_id         UUID NOT NULL REFERENCES file_blobs(id),
    author_id       UUID NOT NULL REFERENCES users(id),
    committed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    commit_message  TEXT NOT NULL DEFAULT '',
    base_version_id UUID REFERENCES file_versions(id),
    native_format   TEXT NOT NULL DEFAULT '',
    measurements    JSONB,
    UNIQUE (file_id, seq)
);

CREATE TABLE IF NOT EXISTS file_revisions (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    item_id      UUID NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    label        TEXT NOT NULL,
    version_id   UUID NOT NULL REFERENCES file_versions(id),
    released_by  UUID REFERENCES users(id),
    released_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    release_note TEXT NOT NULL DEFAULT '',
    scheme       TEXT NOT NULL DEFAULT 'alpha',
    UNIQUE (item_id, label)
);

CREATE TABLE IF NOT EXISTS file_locks (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    item_id          UUID NOT NULL UNIQUE REFERENCES items(id) ON DELETE CASCADE,
    user_id          UUID NOT NULL REFERENCES users(id),
    workstation      TEXT NOT NULL DEFAULT '',
    base_version_id  UUID REFERENCES file_versions(id),
    acquired_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    lease_expires_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS properties (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    item_id    UUID NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    prop_key   TEXT NOT NULL,
    value_type TEXT NOT NULL DEFAULT 'text',
    value      JSONB NOT NULL DEFAULT '{}',
    computed   BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (item_id, prop_key)
);

CREATE TABLE IF NOT EXISTS materials (
    id       UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id   UUID NOT NULL,
    name     TEXT NOT NULL,
    density  DOUBLE PRECISION,
    UNIQUE (org_id, name)
);

CREATE TABLE IF NOT EXISTS audit_events (
    id          BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    org_id      UUID NOT NULL,
    actor_id    UUID,
    agent_id    TEXT NOT NULL DEFAULT '',
    action      TEXT NOT NULL,
    object_type TEXT NOT NULL DEFAULT '',
    object_id   UUID,
    detail      JSONB,
    at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS jobs (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id       UUID NOT NULL,
    kind         TEXT NOT NULL,
    payload      JSONB NOT NULL DEFAULT '{}',
    status       TEXT NOT NULL DEFAULT 'queued',
    attempts     INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 3,
    last_error   TEXT NOT NULL DEFAULT '',
    run_after    TIMESTAMPTZ NOT NULL DEFAULT now(),
    locked_by    TEXT NOT NULL DEFAULT '',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS job_attempts (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    job_id     UUID NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ended_at   TIMESTAMPTZ,
    ok         BOOLEAN,
    error      TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS integration_outbox (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id     UUID NOT NULL,
    kind       TEXT NOT NULL,
    payload    JSONB NOT NULL DEFAULT '{}',
    status     TEXT NOT NULL DEFAULT 'pending',
    attempts   INTEGER NOT NULL DEFAULT 0,
    sent_at    TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Forward-looking reference/BOM tables (populated in Phase 3)
CREATE TABLE IF NOT EXISTS file_references (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    from_item_id         UUID NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    from_feature_id      UUID,
    to_item_id           UUID NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    revision_policy      TEXT NOT NULL DEFAULT 'latest',
    resolved_revision_id UUID REFERENCES file_revisions(id) ON DELETE SET NULL,
    occurrence_count     INTEGER NOT NULL DEFAULT 1,
    suppressed           BOOLEAN NOT NULL DEFAULT FALSE,
    status               TEXT NOT NULL DEFAULT 'resolved',
    UNIQUE (from_item_id, to_item_id, from_feature_id)
);

CREATE TABLE IF NOT EXISTS bom_snapshots (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    revision_id  UUID NOT NULL REFERENCES file_revisions(id) ON DELETE CASCADE,
    generated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    content      JSONB NOT NULL DEFAULT '{}'
);

-- Lifecycle / change management tables (Phase 4)
CREATE TABLE IF NOT EXISTS lifecycle_definitions (
    id     UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL,
    name   TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS lifecycle_states (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    definition_id UUID NOT NULL REFERENCES lifecycle_definitions(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    is_released   BOOLEAN NOT NULL DEFAULT FALSE,
    is_terminal   BOOLEAN NOT NULL DEFAULT FALSE
);
CREATE TABLE IF NOT EXISTS lifecycle_transitions (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    definition_id UUID NOT NULL REFERENCES lifecycle_definitions(id) ON DELETE CASCADE,
    from_state    UUID REFERENCES lifecycle_states(id),
    to_state      UUID NOT NULL REFERENCES lifecycle_states(id)
);
CREATE TABLE IF NOT EXISTS approval_requests (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id       UUID NOT NULL,
    item_id      UUID REFERENCES items(id) ON DELETE CASCADE,
    requested_by UUID REFERENCES users(id),
    status       TEXT NOT NULL DEFAULT 'pending',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS approval_decisions (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id UUID NOT NULL REFERENCES approval_requests(id) ON DELETE CASCADE,
    user_id    UUID REFERENCES users(id),
    decision   TEXT NOT NULL,
    comment    TEXT NOT NULL DEFAULT '',
    decided_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE IF NOT EXISTS change_requests (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id       UUID NOT NULL,
    number       TEXT NOT NULL,
    title        TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    requester_id UUID REFERENCES users(id),
    status       TEXT NOT NULL DEFAULT 'draft',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, number)
);
CREATE TABLE IF NOT EXISTS change_orders (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id     UUID NOT NULL,
    request_id UUID REFERENCES change_requests(id),
    number     TEXT NOT NULL,
    status     TEXT NOT NULL DEFAULT 'draft',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, number)
);
CREATE TABLE IF NOT EXISTS change_affected_items (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    order_id             UUID NOT NULL REFERENCES change_orders(id) ON DELETE CASCADE,
    item_id              UUID NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    previous_revision_id UUID,
    proposed_revision_id UUID,
    UNIQUE (order_id, item_id)
);
CREATE TABLE IF NOT EXISTS change_notices (
    id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id     UUID NOT NULL,
    order_id   UUID NOT NULL REFERENCES change_orders(id) ON DELETE CASCADE,
    number     TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (org_id, number)
);

-- Search vector on items (full-text)
ALTER TABLE items ADD COLUMN IF NOT EXISTS search_tsv tsvector
    GENERATED ALWAYS AS (to_tsvector('english', coalesce(part_number, '') || ' ' || coalesce(description, ''))) STORED;

-- Indexes
CREATE INDEX IF NOT EXISTS idx_users_org ON users(org_id);
CREATE INDEX IF NOT EXISTS idx_tokens_org ON api_tokens(org_id);
CREATE INDEX IF NOT EXISTS idx_projects_org ON projects(org_id);
CREATE INDEX IF NOT EXISTS idx_folders_project ON folders(project_id);
CREATE INDEX IF NOT EXISTS idx_items_org ON items(org_id);
CREATE INDEX IF NOT EXISTS idx_items_project ON items(project_id);
CREATE INDEX IF NOT EXISTS idx_blobs_org ON file_blobs(org_id);
CREATE INDEX IF NOT EXISTS idx_versions_file ON file_versions(file_id);
CREATE INDEX IF NOT EXISTS idx_audit_org_at ON audit_events(org_id, at DESC);
CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status, run_after);
CREATE INDEX IF NOT EXISTS idx_item_part ON items(org_id, part_number);
CREATE INDEX IF NOT EXISTS idx_prop_item ON properties(item_id);
CREATE INDEX IF NOT EXISTS idx_items_search ON items USING GIN (search_tsv);
CREATE INDEX IF NOT EXISTS idx_file_references_from ON file_references(from_item_id);
CREATE INDEX IF NOT EXISTS idx_file_references_to ON file_references(to_item_id);
