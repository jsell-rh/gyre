<script>
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import Modal from '../lib/Modal.svelte';
  import Button from '../lib/Button.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  let { onSelectTask = undefined, workspaceId = '' } = $props();

  let tasks = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let filterAgent = $state('');
  let filterPriority = $state('');

  // New task modal
  let showNewTask = $state(false);
  let taskTitle = $state('');
  let taskDesc = $state('');
  let taskPriority = $state('Medium');
  let taskStatus = $state('backlog');
  let taskCreating = $state(false);

  let columns = $derived([
    { key: 'backlog',     label: $t('tasks.status.backlog'),     colorClass: 'col-backlog' },
    { key: 'in_progress', label: $t('tasks.status.in_progress'), colorClass: 'col-inprogress' },
    { key: 'review',      label: $t('tasks.status.review'),      colorClass: 'col-review' },
    { key: 'done',        label: $t('tasks.status.done'),        colorClass: 'col-done' },
    { key: 'blocked',     label: $t('tasks.status.blocked'),     colorClass: 'col-blocked' },
  ]);

  const priorities = ['Critical', 'High', 'Medium', 'Low'];
  const statuses = ['backlog', 'in_progress', 'review', 'done', 'blocked'];

  const agents = $derived([...new Set(tasks.map((t) => t.assigned_to).filter(Boolean))].sort());

  const filtered = $derived(
    tasks.filter((t) => {
      if (filterAgent && t.assigned_to !== filterAgent) return false;
      if (filterPriority && t.priority !== filterPriority) return false;
      return true;
    })
  );

  function columnTasks(key) {
    return filtered.filter((t) => t.status === key);
  }

  async function loadTasks(wsId = '') {
    try {
      const raw = await api.tasks({ workspaceId: wsId });
      tasks = Array.isArray(raw) ? raw : (raw?.tasks ?? raw ?? []);
    } catch (err) {
      error = err.message;
    }
    loading = false;
  }

  $effect(() => { loadTasks(workspaceId); });

  async function createTask() {
    if (!taskTitle.trim()) return;
    taskCreating = true;
    try {
      await api.createTask({
        title: taskTitle.trim(),
        description: taskDesc.trim() || undefined,
        priority: taskPriority,
        status: taskStatus,
      });
      toastSuccess('Task created');
      showNewTask = false;
      taskTitle = ''; taskDesc = ''; taskPriority = 'Medium'; taskStatus = 'backlog';
      loading = true;
      await loadTasks(workspaceId);
    } catch (e) {
      toastError(e.message);
    }
    taskCreating = false;
  }
</script>

<Modal bind:open={showNewTask} title={$t('tasks.new_task.title')} onsubmit={createTask}>
  <div class="form">
    <label class="form-label">{$t('tasks.new_task.title_label')}
      <input class="form-input" bind:value={taskTitle} placeholder={$t('tasks.new_task.title_placeholder')} />
    </label>
    <label class="form-label">{$t('tasks.new_task.desc_label')}
      <textarea class="form-input form-textarea" bind:value={taskDesc} placeholder={$t('tasks.new_task.desc_placeholder')} rows="3"></textarea>
    </label>
    <label class="form-label">{$t('tasks.new_task.priority_label')}
      <select class="form-input" bind:value={taskPriority}>
        {#each priorities as p}<option value={p}>{p}</option>{/each}
      </select>
    </label>
    <label class="form-label">{$t('tasks.new_task.status_label')}
      <select class="form-input" bind:value={taskStatus}>
        {#each statuses as s}<option value={s}>{s}</option>{/each}
      </select>
    </label>
  </div>
  {#snippet footer()}
    <Button variant="secondary" onclick={() => (showNewTask = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" onclick={createTask} disabled={taskCreating || !taskTitle.trim()}>
      {taskCreating ? $t('common.creating') : $t('common.create') + ' Task'}
    </Button>
  {/snippet}
</Modal>

<div class="page">
  <div class="page-hdr">
    <div>
      <h1 class="page-title">{$t('tasks.title')}</h1>
      <p class="page-desc">{tasks.length} task{tasks.length !== 1 ? 's' : ''} total</p>
    </div>
    <div class="page-actions">
      <div class="filters">
        <select bind:value={filterPriority} class="filter-select">
          <option value="">{$t('tasks.filters.all_priorities')}</option>
          {#each priorities as p}<option value={p}>{p}</option>{/each}
        </select>
        <select bind:value={filterAgent} class="filter-select">
          <option value="">{$t('tasks.filters.all_agents')}</option>
          {#each agents as a}<option value={a}>{a}</option>{/each}
        </select>
      </div>
      <Button variant="primary" onclick={() => (showNewTask = true)}>+ {$t('tasks.new_task.button')}</Button>
    </div>
  </div>

  {#if loading}
    <div class="board">
      {#each columns as col}
        <div class="column">
          <div class="col-header {col.colorClass}">
            <span class="col-title">{col.label}</span>
            <span class="col-count col-count-skel"><Skeleton width="20px" height="1rem" /></span>
          </div>
          <div class="cards">
            {#each Array(2) as _}
              <div class="task-card">
                <Skeleton lines={2} height="0.875rem" />
                <div class="card-meta"><Skeleton width="60px" height="1.1rem" /></div>
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  {:else if error}
    <div class="error-msg">Error: {error}</div>
  {:else}
    <div class="board">
      {#each columns as col}
        {@const colTasks = columnTasks(col.key)}
        <div class="column">
          <div class="col-header {col.colorClass}">
            <span class="col-title">{col.label}</span>
            <span class="col-count">{colTasks.length}</span>
          </div>
          <div class="cards">
            {#each colTasks as task (task.id)}
              <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
              <div
                class="task-card"
                class:clickable={!!onSelectTask}
                onclick={() => onSelectTask?.(task)}
                onkeydown={(e) => e.key === 'Enter' && onSelectTask?.(task)}
                role={onSelectTask ? 'button' : undefined}
                tabindex={onSelectTask ? 0 : undefined}
              >
                <p class="card-title">{task.title}</p>
                <div class="card-meta">
                  <Badge value={task.priority} />
                  {#if task.assigned_to}
                    <span class="assignee">{task.assigned_to}</span>
                  {/if}
                </div>
                {#if task.labels?.length}
                  <div class="label-pills">
                    {#each task.labels as lbl}
                      <span class="label-pill">{lbl}</span>
                    {/each}
                  </div>
                {/if}
                {#if task.pr_link}
                  <a class="pr-link" href={task.pr_link} target="_blank" rel="noreferrer">PR ↗</a>
                {/if}
                {#if task.spec_path}
                  <span class="spec-chip" title={task.spec_path}>📋 spec</span>
                {/if}
              </div>
            {:else}
              <div class="empty-col">
                <EmptyState title="" description="No tasks" />
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    padding: var(--space-6);
    gap: var(--space-4);
  }

  .page-hdr {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
    flex-shrink: 0;
  }

  .page-title {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--color-text);
    margin-bottom: var(--space-1);
  }

  .page-desc { font-size: var(--text-sm); color: var(--color-text-secondary); }

  .page-actions { display: flex; align-items: center; gap: var(--space-3); flex-shrink: 0; }

  .filters { display: flex; gap: var(--space-2); align-items: center; }

  .form { display: flex; flex-direction: column; gap: var(--space-3); }

  .form-label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    font-weight: 500;
  }

  .form-input {
    background: var(--color-bg);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    transition: border-color var(--transition-fast);
  }

  .form-input:focus { outline: none; border-color: var(--color-primary); }

  .form-textarea { resize: vertical; min-height: 5rem; }

  .filter-select {
    background: var(--color-surface);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .board {
    flex: 1;
    overflow-x: auto;
    overflow-y: hidden;
    display: flex;
    gap: var(--space-4);
  }

  .column {
    min-width: 220px;
    flex: 1;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    display: flex;
    flex-direction: column;
    max-height: 100%;
    overflow: hidden;
  }

  .col-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    border-top: 3px solid transparent;
    flex-shrink: 0;
  }

  .col-title {
    font-family: var(--font-display);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .col-count {
    font-size: var(--text-xs);
    font-weight: 600;
    padding: 0.1rem 0.45rem;
    border-radius: 99px;
    background: var(--color-surface-elevated);
    color: var(--color-text-secondary);
  }

  .col-count-skel { background: transparent; }

  /* Column color themes */
  .col-backlog    { border-top-color: var(--color-text-muted); }
  .col-backlog .col-title { color: var(--color-text-muted); }

  .col-inprogress { border-top-color: var(--color-warning); }
  .col-inprogress .col-title { color: var(--color-warning); }

  .col-review     { border-top-color: var(--color-info); }
  .col-review .col-title { color: var(--color-info); }

  .col-done       { border-top-color: var(--color-success); }
  .col-done .col-title { color: var(--color-success); }

  .col-blocked    { border-top-color: var(--color-blocked); }
  .col-blocked .col-title { color: var(--color-blocked); }

  .cards {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .task-card {
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    transition: border-color var(--transition-fast);
  }

  .task-card:hover { border-color: var(--color-border-strong); }
  .task-card.clickable { cursor: pointer; }
  .task-card.clickable:hover { border-color: var(--color-primary); }

  .card-title {
    font-size: var(--text-sm);
    color: var(--color-text);
    line-height: 1.4;
    margin: 0;
  }

  .card-meta { display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; }

  .assignee {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .label-pills { display: flex; gap: var(--space-1); flex-wrap: wrap; }

  .label-pill {
    font-size: 0.7rem;
    color: var(--color-text-secondary);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    padding: 0.05rem 0.4rem;
    border-radius: var(--radius-sm);
  }

  .pr-link {
    font-size: var(--text-xs);
    color: var(--color-link);
    text-decoration: none;
  }

  .pr-link:hover { text-decoration: underline; color: var(--color-link-hover); }

  .spec-chip {
    display: inline-block;
    font-size: var(--text-xs);
    background: var(--color-primary-bg, #1a1a2e);
    border: 1px solid var(--color-primary, #58a6ff);
    color: var(--color-primary, #58a6ff);
    border-radius: var(--radius-full);
    padding: 1px var(--space-2);
    cursor: default;
  }

  .empty-col {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .error-msg {
    padding: var(--space-8);
    color: var(--color-danger);
    text-align: center;
    font-size: var(--text-sm);
  }
</style>
