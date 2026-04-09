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
        description -> Nullable<Text>,
        status -> Text,
        updated_at -> BigInt,
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
        usage_tokens_input -> Nullable<BigInt>,
        usage_tokens_output -> Nullable<BigInt>,
        usage_cost_usd -> Nullable<Double>,
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
        cancelled_at -> Nullable<BigInt>,
        cancelled_reason -> Nullable<Text>,
        task_type -> Nullable<Text>,
        order -> Nullable<Integer>,
        depends_on -> Text,
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
        reverted_at -> Nullable<BigInt>,
        revert_mr_id -> Nullable<Text>,
        spec_ref -> Nullable<Text>,
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
        display_name -> Nullable<Text>,
        timezone -> Nullable<Text>,
        locale -> Nullable<Text>,
    }
}

diesel::table! {
    user_notification_preferences (user_id, notification_type) {
        user_id -> Text,
        notification_type -> Text,
        enabled -> Integer,
    }
}

diesel::table! {
    user_tokens (id) {
        id -> Text,
        user_id -> Text,
        name -> Text,
        token_hash -> Text,
        created_at -> BigInt,
        last_used_at -> Nullable<BigInt>,
        expires_at -> Nullable<BigInt>,
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
        trust_level -> Text,
        llm_model -> Nullable<Text>,
        created_at -> BigInt,
        compute_target_id -> Nullable<Text>,
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
        workspace_id -> Text,
        user_id -> Text,
        notification_type -> Text,
        priority -> Integer,
        title -> Text,
        body -> Nullable<Text>,
        entity_ref -> Nullable<Text>,
        repo_id -> Nullable<Text>,
        resolved_at -> Nullable<BigInt>,
        dismissed_at -> Nullable<BigInt>,
        created_at -> BigInt,
        tenant_id -> Text,
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
        immutable -> Integer,
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
        rejected_at -> Nullable<BigInt>,
        rejected_reason -> Nullable<Text>,
        rejected_by -> Nullable<Text>,
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
        enforce_manifest -> Integer,
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
        repo_id -> Nullable<Text>,
        workspace_id -> Nullable<Text>,
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

diesel::table! {
    meta_spec_sets (workspace_id) {
        workspace_id -> Text,
        json -> Text,
        updated_at -> BigInt,
    }
}

diesel::table! {
    budget_call_records (id) {
        id -> Text,
        tenant_id -> Text,
        workspace_id -> Text,
        repo_id -> Nullable<Text>,
        agent_id -> Nullable<Text>,
        task_id -> Nullable<Text>,
        usage_type -> Text,
        input_tokens -> BigInt,
        output_tokens -> BigInt,
        cost_usd -> Double,
        model -> Text,
        timestamp -> BigInt,
    }
}

diesel::table! {
    gate_traces (id) {
        id -> Text,
        mr_id -> Text,
        gate_run_id -> Text,
        commit_sha -> Text,
        captured_at -> BigInt,
        tenant_id -> Text,
        permanent -> Integer,
    }
}

diesel::table! {
    trace_spans (span_id, gate_trace_id) {
        span_id -> Text,
        gate_trace_id -> Text,
        parent_span_id -> Nullable<Text>,
        operation_name -> Text,
        service_name -> Text,
        kind -> Text,
        start_time -> BigInt,
        duration_us -> BigInt,
        attributes -> Text,
        input_summary -> Nullable<Text>,
        output_summary -> Nullable<Text>,
        payload_blob -> Nullable<Binary>,
        status -> Text,
        graph_node_id -> Nullable<Text>,
    }
}

diesel::joinable!(trace_spans -> gate_traces (gate_trace_id));

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
    user_notification_preferences,
    user_tokens,
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
    meta_spec_sets,
    budget_call_records,
    user_workspace_state,
    gate_traces,
    trace_spans,
    conversations,
    turn_commit_links,
    llm_function_configs,
    graph_nodes,
    graph_edges,
    graph_deltas,
    compute_targets,
    meta_specs,
    meta_spec_versions,
    meta_spec_bindings,
    trust_anchors,
    key_bindings,
    chain_attestations,
);

diesel::table! {
    user_workspace_state (user_id, workspace_id) {
        user_id -> Text,
        workspace_id -> Text,
        last_seen_at -> BigInt,
    }
}

diesel::table! {
    conversations (sha) {
        sha -> Text,
        agent_id -> Text,
        workspace_id -> Text,
        blob -> Nullable<Binary>,
        file_path -> Nullable<Text>,
        created_at -> BigInt,
        tenant_id -> Text,
    }
}

diesel::table! {
    turn_commit_links (id) {
        id -> Text,
        agent_id -> Text,
        turn_number -> Integer,
        commit_sha -> Text,
        files_changed -> Text,
        conversation_sha -> Nullable<Text>,
        timestamp -> BigInt,
        tenant_id -> Text,
    }
}

diesel::table! {
    graph_nodes (id) {
        id -> Text,
        repo_id -> Text,
        node_type -> Text,
        name -> Text,
        qualified_name -> Text,
        file_path -> Text,
        line_start -> Integer,
        line_end -> Integer,
        visibility -> Text,
        doc_comment -> Nullable<Text>,
        spec_path -> Nullable<Text>,
        spec_confidence -> Text,
        last_modified_sha -> Text,
        last_modified_by -> Nullable<Text>,
        last_modified_at -> BigInt,
        created_sha -> Text,
        created_at -> BigInt,
        complexity -> Nullable<Integer>,
        churn_count_30d -> Integer,
        test_coverage -> Nullable<Double>,
        first_seen_at -> BigInt,
        last_seen_at -> BigInt,
        deleted_at -> Nullable<BigInt>,
        test_node -> Bool,
    }
}

diesel::table! {
    graph_edges (id) {
        id -> Text,
        repo_id -> Text,
        source_id -> Text,
        target_id -> Text,
        edge_type -> Text,
        metadata -> Nullable<Text>,
        first_seen_at -> BigInt,
        last_seen_at -> BigInt,
        deleted_at -> Nullable<BigInt>,
    }
}

diesel::table! {
    graph_deltas (id) {
        id -> Text,
        repo_id -> Text,
        commit_sha -> Text,
        timestamp -> BigInt,
        agent_id -> Nullable<Text>,
        spec_ref -> Nullable<Text>,
        delta_json -> Text,
    }
}

diesel::table! {
    prompt_templates (id) {
        id -> Text,
        workspace_id -> Nullable<Text>,
        function_key -> Text,
        content -> Text,
        created_by -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    llm_function_configs (id) {
        id -> Text,
        workspace_id -> Nullable<Text>,
        function_key -> Text,
        model_name -> Text,
        max_tokens -> Nullable<Integer>,
        updated_by -> Text,
        updated_at -> BigInt,
    }
}

diesel::table! {
    compute_targets (id) {
        id -> Text,
        tenant_id -> Text,
        name -> Text,
        target_type -> Text,
        config -> Text,
        is_default -> Integer,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    meta_specs (id) {
        id -> Text,
        kind -> Text,
        name -> Text,
        scope -> Text,
        scope_id -> Nullable<Text>,
        prompt -> Text,
        version -> Integer,
        content_hash -> Text,
        required -> Integer,
        approval_status -> Text,
        approved_by -> Nullable<Text>,
        approved_at -> Nullable<BigInt>,
        created_by -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    meta_spec_versions (id) {
        id -> Text,
        meta_spec_id -> Text,
        version -> Integer,
        prompt -> Text,
        content_hash -> Text,
        created_at -> BigInt,
    }
}

diesel::table! {
    meta_spec_bindings (id) {
        id -> Text,
        spec_id -> Text,
        meta_spec_id -> Text,
        pinned_version -> Integer,
        created_at -> BigInt,
    }
}

diesel::table! {
    saved_views (id) {
        id -> Text,
        repo_id -> Text,
        workspace_id -> Text,
        tenant_id -> Text,
        name -> Text,
        description -> Nullable<Text>,
        query_json -> Text,
        created_by -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
        is_system -> Bool,
    }
}

diesel::table! {
    trust_anchors (tenant_id, id) {
        id -> Text,
        tenant_id -> Text,
        issuer -> Text,
        jwks_uri -> Text,
        anchor_type -> Text,
        constraints_json -> Text,
        created_at -> BigInt,
    }
}

diesel::table! {
    key_bindings (id) {
        id -> Text,
        user_identity -> Text,
        tenant_id -> Text,
        public_key -> Binary,
        issuer -> Text,
        trust_anchor_id -> Text,
        issued_at -> BigInt,
        expires_at -> BigInt,
        user_signature -> Binary,
        platform_countersign -> Binary,
        revoked_at -> Nullable<BigInt>,
    }
}

diesel::table! {
    chain_attestations (id) {
        id -> Text,
        input_type -> Text,
        input_json -> Text,
        output_json -> Text,
        metadata_json -> Text,
        parent_ref -> Nullable<Text>,
        chain_depth -> Integer,
        workspace_id -> Text,
        repo_id -> Text,
        task_id -> Text,
        agent_id -> Text,
        created_at -> BigInt,
        tenant_id -> Text,
        commit_sha -> Text,
    }
}
