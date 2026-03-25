// @generated — hand-written for M15.1 (diesel print-schema equivalent)
// Matches migrations/2024-01-01-000001_initial_schema/up.sql

diesel::table! {
    repositories (id) {
        id -> Text,
        name -> Text,
        path -> Text,
        default_branch -> Text,
        created_at -> BigInt,
        is_mirror -> Integer,
        mirror_url -> Nullable<Text>,
        mirror_interval_secs -> Nullable<BigInt>,
        last_mirror_sync -> Nullable<BigInt>,
        tenant_id -> Text,
        workspace_id -> Text,
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
        spawned_by -> Nullable<Text>,
        workspace_id -> Text,
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
        workspace_id -> Text,
        spec_path -> Nullable<Text>,
        repo_id -> Text,
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
        depends_on -> Text,
        atomic_group -> Nullable<Text>,
        workspace_id -> Text,
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
        mesh_ip -> Nullable<Text>,
        is_stale -> Bool,
    }
}

diesel::table! {
    spawn_log (id) {
        id -> Text,
        agent_id -> Text,
        step -> Text,
        status -> Text,
        detail -> Nullable<Text>,
        occurred_at -> BigInt,
    }
}

diesel::table! {
    revoked_tokens (token_hash) {
        token_hash -> Text,
        agent_id -> Text,
        revoked_at -> BigInt,
    }
}

diesel::table! {
    workspaces (id) {
        id -> Text,
        tenant_id -> Text,
        name -> Text,
        slug -> Text,
        description -> Nullable<Text>,
        budget -> Nullable<Text>,
        max_repos -> Nullable<Integer>,
        max_agents_per_repo -> Nullable<Integer>,
        created_at -> BigInt,
    }
}

diesel::table! {
    personas (id) {
        id -> Text,
        name -> Text,
        slug -> Text,
        scope -> Text,
        system_prompt -> Text,
        capabilities -> Text,
        protocols -> Text,
        model -> Nullable<Text>,
        temperature -> Nullable<Double>,
        max_tokens -> Nullable<Integer>,
        budget -> Nullable<Text>,
        created_at -> BigInt,
        version -> Integer,
        content_hash -> Text,
        owner -> Nullable<Text>,
        approval_status -> Text,
        approved_by -> Nullable<Text>,
        approved_at -> Nullable<BigInt>,
        updated_at -> BigInt,
    }
}

diesel::table! {
    teams (id) {
        id -> Text,
        workspace_id -> Text,
        name -> Text,
        description -> Nullable<Text>,
        member_ids -> Text,
        created_at -> BigInt,
    }
}

diesel::table! {
    workspace_memberships (id) {
        id -> Text,
        user_id -> Text,
        workspace_id -> Text,
        role -> Text,
        invited_by -> Text,
        accepted -> Integer,
        accepted_at -> Nullable<BigInt>,
        created_at -> BigInt,
    }
}

diesel::table! {
    notifications (id) {
        id -> Text,
        user_id -> Text,
        notification_type -> Text,
        title -> Text,
        body -> Text,
        entity_type -> Nullable<Text>,
        entity_id -> Nullable<Text>,
        priority -> Text,
        action_url -> Nullable<Text>,
        read -> Integer,
        read_at -> Nullable<BigInt>,
        created_at -> BigInt,
    }
}

diesel::table! {
    policies (id) {
        id -> Text,
        name -> Text,
        description -> Text,
        scope -> Text,
        scope_id -> Nullable<Text>,
        priority -> Integer,
        effect -> Text,
        conditions -> Text,
        actions -> Text,
        resource_types -> Text,
        enabled -> Integer,
        built_in -> Integer,
        created_by -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    policy_decisions (request_id) {
        request_id -> Text,
        subject_id -> Text,
        subject_type -> Text,
        action -> Text,
        resource_type -> Text,
        resource_id -> Text,
        decision -> Text,
        matched_policy -> Nullable<Text>,
        evaluated_policies -> Integer,
        evaluation_ms -> Double,
        evaluated_at -> BigInt,
    }
}

diesel::table! {
    spec_approvals (id) {
        id -> Text,
        spec_path -> Text,
        spec_sha -> Text,
        approver_id -> Text,
        signature -> Nullable<Text>,
        approved_at -> BigInt,
        revoked_at -> Nullable<BigInt>,
        revoked_by -> Nullable<Text>,
        revocation_reason -> Nullable<Text>,
    }
}

diesel::table! {
    dependency_edges (id) {
        id -> Text,
        source_repo_id -> Text,
        target_repo_id -> Text,
        dependency_type -> Text,
        source_artifact -> Text,
        target_artifact -> Text,
        version_pinned -> Nullable<Text>,
        version_drift -> Nullable<Integer>,
        detection_method -> Text,
        status -> Text,
        detected_at -> BigInt,
        last_verified_at -> BigInt,
    }
}

diesel::table! {
    budget_configs (entity_key) {
        entity_key -> Text,
        max_tokens_per_day -> Nullable<BigInt>,
        max_cost_per_day -> Nullable<Double>,
        max_concurrent_agents -> Nullable<Integer>,
        max_agent_lifetime_secs -> Nullable<BigInt>,
        updated_at -> BigInt,
    }
}

diesel::table! {
    kv_store (namespace, key) {
        namespace -> Text,
        key -> Text,
        value_json -> Text,
        updated_at -> BigInt,
    }
}

diesel::table! {
    quality_gates (id) {
        id -> Text,
        repo_id -> Text,
        name -> Text,
        gate_type -> Text,
        command -> Nullable<Text>,
        required_approvals -> Nullable<Integer>,
        persona -> Nullable<Text>,
        required -> Integer,
        created_at -> BigInt,
    }
}

diesel::table! {
    gate_results (id) {
        id -> Text,
        gate_id -> Text,
        mr_id -> Text,
        status -> Text,
        output -> Nullable<Text>,
        started_at -> Nullable<BigInt>,
        finished_at -> Nullable<BigInt>,
    }
}

diesel::table! {
    repo_push_gates (repo_id) {
        repo_id -> Text,
        gate_names -> Text,
    }
}

diesel::table! {
    spec_policies (repo_id) {
        repo_id -> Text,
        require_spec_ref -> Integer,
        require_approved_spec -> Integer,
        warn_stale_spec -> Integer,
        require_current_spec -> Integer,
    }
}

diesel::table! {
    attestation_bundles (mr_id) {
        mr_id -> Text,
        attestation -> Text,
        signature -> Text,
        signing_key_id -> Text,
    }
}

diesel::table! {
    container_audit_records (agent_id) {
        agent_id -> Text,
        container_id -> Text,
        image -> Text,
        image_hash -> Nullable<Text>,
        runtime -> Text,
        started_at -> BigInt,
        stopped_at -> Nullable<BigInt>,
        exit_code -> Nullable<Integer>,
    }
}

diesel::table! {
    spec_ledger_entries (path) {
        path -> Text,
        title -> Text,
        owner -> Text,
        kind -> Nullable<Text>,
        current_sha -> Text,
        approval_mode -> Text,
        approval_status -> Text,
        linked_tasks -> Text,
        linked_mrs -> Text,
        drift_status -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    budget_usages (entity_key) {
        entity_key -> Text,
        entity_type -> Text,
        entity_id -> Text,
        tokens_used_today -> BigInt,
        cost_today -> Double,
        active_agents -> Integer,
        period_start -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    spec_approval_events (id) {
        id -> Text,
        spec_path -> Text,
        spec_sha -> Text,
        approver_type -> Text,
        approver_id -> Text,
        persona -> Nullable<Text>,
        approved_at -> BigInt,
        revoked_at -> Nullable<BigInt>,
        revoked_by -> Nullable<Text>,
        revocation_reason -> Nullable<Text>,
    }
}

diesel::table! {
    tenants (id) {
        id -> Text,
        name -> Text,
        slug -> Text,
        oidc_issuer -> Nullable<Text>,
        budget -> Nullable<Text>,
        max_workspaces -> Nullable<Integer>,
        created_at -> BigInt,
    }
}

diesel::table! {
    messages (id) {
        id -> Text,
        tenant_id -> Text,
        from_type -> Text,
        from_id -> Nullable<Text>,
        workspace_id -> Text,
        to_type -> Text,
        to_id -> Nullable<Text>,
        kind -> Text,
        payload -> Nullable<Text>,
        created_at -> BigInt,
        signature -> Nullable<Text>,
        key_id -> Nullable<Text>,
        acknowledged -> Integer,
        ack_reason -> Nullable<Text>,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
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
    spawn_log,
    revoked_tokens,
    workspaces,
    personas,
    teams,
    workspace_memberships,
    notifications,
    policies,
    policy_decisions,
    spec_approvals,
    dependency_edges,
    budget_configs,
    kv_store,
    budget_usages,
    quality_gates,
    gate_results,
    repo_push_gates,
    spec_policies,
    attestation_bundles,
    container_audit_records,
    spec_ledger_entries,
    spec_approval_events,
    tenants,
    messages,
);
