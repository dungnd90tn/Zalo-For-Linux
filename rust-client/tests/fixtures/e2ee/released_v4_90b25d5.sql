-- Exact schema SQL extracted from repository commit 90b25d5. No profile data.
PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;
            BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS conversations (
                account_id TEXT NOT NULL, thread_id TEXT NOT NULL, title TEXT NOT NULL,
                last_message_at_ms INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (account_id, thread_id));
            CREATE TABLE IF NOT EXISTS messages (
                account_id TEXT NOT NULL, thread_id TEXT NOT NULL, message_id TEXT NOT NULL,
                sender TEXT NOT NULL, body TEXT NOT NULL, sent_at_ms INTEGER NOT NULL,
                PRIMARY KEY (account_id, thread_id, message_id),
                FOREIGN KEY (account_id, thread_id) REFERENCES conversations(account_id, thread_id));
            CREATE INDEX IF NOT EXISTS message_page ON messages(account_id, thread_id, sent_at_ms DESC, message_id DESC);
            CREATE INDEX IF NOT EXISTS conversation_recent ON conversations(account_id, last_message_at_ms DESC, thread_id);
            CREATE TABLE IF NOT EXISTS outgoing (
                account_id TEXT NOT NULL, thread_id TEXT NOT NULL, client_id TEXT NOT NULL,
                body TEXT NOT NULL, created_at_ms INTEGER NOT NULL, state TEXT NOT NULL,
                server_id TEXT, PRIMARY KEY(account_id,client_id));
            CREATE INDEX IF NOT EXISTS outgoing_thread ON outgoing(account_id,thread_id,state);
            CREATE INDEX IF NOT EXISTS outgoing_server ON outgoing(account_id,thread_id,server_id);
            CREATE TABLE IF NOT EXISTS outgoing_counters(account_id TEXT PRIMARY KEY,last_id INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS receive_cursors(account_id TEXT NOT NULL,queue TEXT NOT NULL,last_id TEXT NOT NULL,
                PRIMARY KEY(account_id,queue));
            
CREATE TABLE IF NOT EXISTS directory_entries(
        account_id TEXT NOT NULL,thread_id TEXT NOT NULL,remote_id TEXT NOT NULL,
        is_group INTEGER NOT NULL,display_name TEXT NOT NULL,search_name TEXT NOT NULL,avatar_url TEXT,
        is_friend INTEGER,is_blocked INTEGER,member_count INTEGER,
        observed_at_ms INTEGER NOT NULL,PRIMARY KEY(account_id,thread_id));
        CREATE INDEX IF NOT EXISTS directory_names ON directory_entries(account_id,is_group,display_name,thread_id);
        CREATE INDEX IF NOT EXISTS directory_search ON directory_entries(account_id,is_group,search_name,thread_id);
CREATE TABLE IF NOT EXISTS e2ee_accounts(account TEXT PRIMARY KEY,revision INTEGER NOT NULL DEFAULT 0,next_local INTEGER NOT NULL DEFAULT 0,next_peer INTEGER NOT NULL DEFAULT 0,current_local INTEGER);
    CREATE TABLE IF NOT EXISTS e2ee_local_epochs(account TEXT NOT NULL,epoch INTEGER NOT NULL,platform INTEGER NOT NULL,registration_id INTEGER NOT NULL,fingerprint BLOB NOT NULL,session_ts INTEGER NOT NULL,next_prekey INTEGER NOT NULL,PRIMARY KEY(account,epoch));
    CREATE TABLE IF NOT EXISTS e2ee_peer_epochs(account TEXT NOT NULL,local_epoch INTEGER NOT NULL,peer_epoch INTEGER NOT NULL,uid TEXT NOT NULL,platform INTEGER NOT NULL,registration_id INTEGER NOT NULL,fingerprint BLOB NOT NULL,session_ts INTEGER NOT NULL,active INTEGER NOT NULL,PRIMARY KEY(account,local_epoch,peer_epoch));
    CREATE UNIQUE INDEX IF NOT EXISTS e2ee_active_peer ON e2ee_peer_epochs(account,local_epoch,uid,platform) WHERE active=1;
    CREATE TABLE IF NOT EXISTS e2ee_records(account TEXT NOT NULL,key TEXT NOT NULL,value BLOB NOT NULL,PRIMARY KEY(account,key));
    CREATE TABLE IF NOT EXISTS e2ee_inputs(account TEXT NOT NULL,queue TEXT NOT NULL,server_id TEXT NOT NULL,cipher_digest BLOB NOT NULL,operation_id INTEGER NOT NULL,PRIMARY KEY(account,queue,server_id),UNIQUE(account,cipher_digest));
    CREATE TABLE IF NOT EXISTS e2ee_operations(account TEXT NOT NULL,operation_id INTEGER NOT NULL,digest BLOB NOT NULL,input_digest BLOB NOT NULL,revision INTEGER NOT NULL,queue TEXT,cursor INTEGER,PRIMARY KEY(account,operation_id));
    CREATE TABLE IF NOT EXISTS e2ee_outbox(account TEXT NOT NULL,client_id INTEGER NOT NULL,thread TEXT NOT NULL,local_epoch INTEGER NOT NULL,platform INTEGER NOT NULL,body TEXT NOT NULL,created_ms INTEGER NOT NULL,state TEXT NOT NULL,ciphertext BLOB,metadata BLOB,server_id TEXT,operation_id INTEGER,crypto_revision INTEGER,peer_bindings TEXT,payload_hash BLOB,PRIMARY KEY(account,client_id));
    CREATE UNIQUE INDEX IF NOT EXISTS e2ee_server_message ON e2ee_outbox(account,thread,server_id) WHERE server_id IS NOT NULL;
    CREATE INDEX IF NOT EXISTS e2ee_outbox_state ON e2ee_outbox(account,state,client_id);
PRAGMA user_version=4; COMMIT;
