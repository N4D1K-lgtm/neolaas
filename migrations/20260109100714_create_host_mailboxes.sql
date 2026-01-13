-- Create host_mailboxes table for host communication during provisioning
CREATE TABLE IF NOT EXISTS host_mailboxes (
    id UUID PRIMARY KEY,
    host_id UUID NOT NULL,
    message_type VARCHAR(100) NOT NULL,
    payload JSONB NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    delivered_at TIMESTAMPTZ,

    CONSTRAINT host_mailboxes_status_check CHECK (status IN ('pending', 'delivered', 'failed'))
);

CREATE INDEX IF NOT EXISTS idx_host_mailboxes_host_id ON host_mailboxes(host_id);
CREATE INDEX IF NOT EXISTS idx_host_mailboxes_status ON host_mailboxes(status);
CREATE INDEX IF NOT EXISTS idx_host_mailboxes_created_at ON host_mailboxes(created_at);

COMMENT ON TABLE host_mailboxes IS 'Message queue for host communication during provisioning';
COMMENT ON COLUMN host_mailboxes.status IS 'Message delivery status: pending, delivered, or failed';
