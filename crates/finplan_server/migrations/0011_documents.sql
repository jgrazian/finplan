-- Documents attached to an AI-guided draft (documents, api::documents).
--
-- Original files are never stored. A row holds only the text extracted from the
-- upload after redaction (SSNs, account numbers, addresses, ... masked), its
-- SHA-256 and small structured data parsed from it. A file that has no text
-- layer (a screenshot, a scanned PDF) is held for the model outside the
-- database, in a per-draft temp directory, and removed with the draft; the row
-- only records that it is waiting there. So database backups never hold a
-- statement or a tax return.

-- Whether a draft's documents outlive Create & run ("keep with this plan").
ALTER TABLE scenarios ADD COLUMN retain_documents INTEGER NOT NULL DEFAULT 0;

CREATE TABLE documents (
    id            INTEGER PRIMARY KEY,
    user_id       TEXT    NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Deleting a draft (cancel, replace, sweep) deletes its documents.
    scenario_id   INTEGER NOT NULL REFERENCES scenarios(id) ON DELETE CASCADE,
    filename      TEXT    NOT NULL,
    mime          TEXT    NOT NULL,
    sha256        TEXT    NOT NULL,
    -- Size of the upload, for the draft's byte limit; the bytes are not kept.
    bytes         INTEGER NOT NULL,
    -- Pages of `text` (separated by form feeds) and what the upload counts
    -- for against the draft's page limit (PDF pages plus images).
    pages         INTEGER NOT NULL,
    page_units    INTEGER NOT NULL,
    -- Redacted extracted text, one page after another, separated by U+000C.
    text          TEXT    NOT NULL,
    kind          TEXT    NOT NULL CHECK (kind IN (
                      'bank_statement', 'brokerage_statement', 'retirement_statement',
                      'pay_stub', 'tax_return', 'transactions', 'image', 'other')),
    status        TEXT    NOT NULL CHECK (status IN (
                      'parsed', 'needs_ocr', 'image_unredacted', 'failed')),
    -- Why a document is not `parsed`, in words for the person.
    note          TEXT,
    retain        INTEGER NOT NULL DEFAULT 0,
    -- The one birth date found, ISO; every birth date is masked in `text`.
    birth_date_hint TEXT,
    -- Balances, positions and transactions read from the document (JSON).
    data_json     TEXT,
    created_at    TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (scenario_id, sha256)
);
CREATE INDEX documents_by_scenario ON documents (scenario_id, id);
