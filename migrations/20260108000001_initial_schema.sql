-- Initial schema for neolaas

-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Hosts table
CREATE TABLE hosts (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(255) NOT NULL UNIQUE,
    mac_address VARCHAR(17) NOT NULL UNIQUE,
    ip_address VARCHAR(45),
    host_type VARCHAR(10) NOT NULL CHECK (host_type IN ('dev', 'prod')),
    status VARCHAR(20) NOT NULL CHECK (status IN ('provisioning', 'ready', 'in_use', 'maintenance', 'failed')),
    metadata JSONB DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_hosts_host_type ON hosts(host_type);
CREATE INDEX idx_hosts_status ON hosts(status);
CREATE INDEX idx_hosts_mac_address ON hosts(mac_address);

-- Resource locks table
CREATE TABLE resource_locks (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    resource_id VARCHAR(255) NOT NULL UNIQUE,
    holder VARCHAR(255) NOT NULL,
    acquired_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ,
    metadata JSONB DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_locks_resource_id ON resource_locks(resource_id);
CREATE INDEX idx_locks_holder ON resource_locks(holder);
CREATE INDEX idx_locks_expires_at ON resource_locks(expires_at);

-- Hardware inventory table
CREATE TABLE inventory (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name VARCHAR(255) NOT NULL UNIQUE,
    item_type VARCHAR(50) NOT NULL,
    vendor VARCHAR(100),
    model VARCHAR(100),
    serial_number VARCHAR(100),
    management_ip VARCHAR(45),
    status VARCHAR(20) NOT NULL CHECK (status IN ('active', 'inactive', 'maintenance', 'retired')),
    metadata JSONB DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_inventory_item_type ON inventory(item_type);
CREATE INDEX idx_inventory_status ON inventory(status);

-- Switches table (specific hardware type)
CREATE TABLE switches (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    inventory_id UUID NOT NULL REFERENCES inventory(id) ON DELETE CASCADE,
    hostname VARCHAR(255) NOT NULL UNIQUE,
    management_ip VARCHAR(45) NOT NULL,
    vendor VARCHAR(100) NOT NULL,
    model VARCHAR(100),
    ports_count INTEGER,
    metadata JSONB DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_switches_inventory_id ON switches(inventory_id);

-- Host mailboxes table (for host communication)
CREATE TABLE host_mailboxes (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    host_id UUID NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    message_type VARCHAR(50) NOT NULL,
    payload JSONB NOT NULL,
    status VARCHAR(20) NOT NULL CHECK (status IN ('pending', 'delivered', 'failed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    delivered_at TIMESTAMPTZ
);

CREATE INDEX idx_mailboxes_host_id ON host_mailboxes(host_id);
CREATE INDEX idx_mailboxes_status ON host_mailboxes(status);
CREATE INDEX idx_mailboxes_created_at ON host_mailboxes(created_at);

-- Deployment history table
CREATE TABLE deployment_history (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    host_id UUID NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    version VARCHAR(100),
    image VARCHAR(255),
    status VARCHAR(20) NOT NULL CHECK (status IN ('pending', 'in_progress', 'completed', 'failed', 'rolled_back')),
    initiated_by VARCHAR(255),
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    error_message TEXT,
    metadata JSONB DEFAULT '{}'::jsonb
);

CREATE INDEX idx_deployment_history_host_id ON deployment_history(host_id);
CREATE INDEX idx_deployment_history_status ON deployment_history(status);
CREATE INDEX idx_deployment_history_started_at ON deployment_history(started_at);

-- Function to update updated_at timestamp
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Triggers for updated_at
CREATE TRIGGER update_hosts_updated_at BEFORE UPDATE ON hosts
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_inventory_updated_at BEFORE UPDATE ON inventory
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_switches_updated_at BEFORE UPDATE ON switches
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
