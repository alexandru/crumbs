CREATE INDEX oauth_clients_by_creation ON oauth_clients (created_at, id);

CREATE INDEX oauth_authorizations_by_session ON oauth_authorizations (session_id)
    WHERE session_id IS NOT NULL;

CREATE INDEX oauth_authorizations_unredeemed_by_expiry ON oauth_authorizations (expires_at)
    WHERE session_id IS NULL;

DROP INDEX oauth_authorizations_by_expiry;

CREATE INDEX crumbs_foods_custom_by_id ON crumbs_foods (source, id)
    WHERE source = 'custom' AND dataset = 'custom';

CREATE INDEX profiles_by_user ON profiles (user_id);

CREATE INDEX profile_logs_profile_day_time_id
    ON profile_logs (profile_id, day ASC, time DESC, id DESC);

DROP INDEX profile_logs_profile_day_id;
