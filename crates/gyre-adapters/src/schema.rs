// @generated — hand-written for M15.1 (diesel print-schema equivalent)
// Matches migrations/2024-01-01-000001_initial_schema/up.sql

diesel::table! {
    projects (id) {
        id -> Text,
        name -> Text,
        description -> Nullable<Text>,
        created_at -> BigInt,
        updated_at -> BigInt,
        tenant_id -> Text,
    }
}

diesel::table! {
    repositories (id) {
        id -> Text,
        project_id -> Text,
        name -> Text,
        path -> Text,
        default_branch -> Text,
        created_at -> BigInt,
        is_mirror -> Integer,
        mirror_url -> Nullable<Text>,
        mirror_interval_secs -> Nullable<BigInt>,
        last_mirror_sync -> Nullable<BigInt>,
        tenant_id -> Text,
    }
}

diesel::table! {
    agents (id) {
        id -> Text,
        name -> Text,
        status -> Text,
        parent_id -> Nullable<Text>,
        current_task_id -> Nullable<Text>,
        lifetime_budget_secs -> Nullable<BigInt>,
        spawned_at -> BigInt,
        last_heartbeat -> Nullable<BigInt>,
        tenant_id -> Text,
    }
}

diesel::table! {
    tasks (id) {
        id -> Text,
        title -> Text,
        description -> Nullable<Text>,
        status -> Text,
        priority -> Text,
        assigned_to -> Nullable<Text>,
        parent_task_id -> Nullable<Text>,
        labels -> Text,
        branch -> Nullable<Text>,
        pr_link -> Nullable<Text>,
        created_at -> BigInt,
        updated_at -> BigInt,
        tenant_id -> Text,
    }
}

diesel::table! {
    merge_requests (id) {
        id -> Text,
        repository_id -> Text,
        title -> Text,
        source_branch -> Text,
        target_branch -> Text,
        status -> Text,
        author_agent_id -> Nullable<Text>,
        reviewers -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
        diff_files_changed -> Nullable<BigInt>,
        diff_insertions -> Nullable<BigInt>,
        diff_deletions -> Nullable<BigInt>,
        has_conflicts -> Nullable<Integer>,
        tenant_id -> Text,
    }
}

diesel::table! {
    activity_events (id) {
        id -> Text,
        agent_id -> Text,
        event_type -> Text,
        description -> Text,
        timestamp -> BigInt,
        tenant_id -> Text,
    }
}

diesel::table! {
    review_comments (id) {
        id -> Text,
        merge_request_id -> Text,
        author_agent_id -> Text,
        body -> Text,
        file_path -> Nullable<Text>,
        line_number -> Nullable<Integer>,
        created_at -> BigInt,
    }
}

diesel::table! {
    reviews (id) {
        id -> Text,
        merge_request_id -> Text,
        reviewer_agent_id -> Text,
        decision -> Text,
        body -> Nullable<Text>,
        created_at -> BigInt,
    }
}

diesel::table! {
    merge_queue (id) {
        id -> Text,
        merge_request_id -> Text,
        priority -> Integer,
        status -> Text,
        enqueued_at -> BigInt,
        processed_at -> Nullable<BigInt>,
        error_message -> Nullable<Text>,
    }
}

diesel::table! {
    agent_commits (id) {
        id -> Text,
        agent_id -> Text,
        repository_id -> Text,
        commit_sha -> Text,
        branch -> Text,
        timestamp -> BigInt,
        task_id -> Nullable<Text>,
        ralph_step -> Nullable<Text>,
        spawned_by_user_id -> Nullable<Text>,
        parent_agent_id -> Nullable<Text>,
        model_context -> Nullable<Text>,
        attestation_level -> Nullable<Text>,
    }
}

diesel::table! {
    agent_worktrees (id) {
        id -> Text,
        agent_id -> Text,
        repository_id -> Text,
        task_id -> Nullable<Text>,
        branch -> Text,
        path -> Text,
        created_at -> BigInt,
    }
}

diesel::table! {
    users (id) {
        id -> Text,
        external_id -> Text,
        name -> Text,
        email -> Nullable<Text>,
        roles -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    api_keys (key) {
        key -> Text,
        user_id -> Text,
        name -> Text,
        created_at -> BigInt,
    }
}

diesel::table! {
    analytics_events (id) {
        id -> Text,
        event_name -> Text,
        agent_id -> Nullable<Text>,
        properties -> Text,
        timestamp -> BigInt,
        tenant_id -> Text,
    }
}

diesel::table! {
    cost_entries (id) {
        id -> Text,
        agent_id -> Text,
        task_id -> Nullable<Text>,
        cost_type -> Text,
        amount -> Double,
        currency -> Text,
        timestamp -> BigInt,
        tenant_id -> Text,
    }
}

diesel::table! {
    audit_events (id) {
        id -> Text,
        agent_id -> Text,
        event_type -> Text,
        path -> Nullable<Text>,
        details -> Text,
        pid -> Nullable<Integer>,
        timestamp -> BigInt,
    }
}

diesel::table! {
    network_peers (id) {
        id -> Text,
        agent_id -> Text,
        wireguard_pubkey -> Text,
        endpoint -> Nullable<Text>,
        allowed_ips -> Text,
        registered_at -> BigInt,
        last_seen -> Nullable<BigInt>,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    projects,
    repositories,
    agents,
    tasks,
    merge_requests,
    activity_events,
    review_comments,
    reviews,
    merge_queue,
    agent_commits,
    agent_worktrees,
    users,
    api_keys,
    analytics_events,
    cost_entries,
    audit_events,
    network_peers,
);
