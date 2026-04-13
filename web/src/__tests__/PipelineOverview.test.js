import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/svelte';
import PipelineOverview from '../components/PipelineOverview.svelte';

describe('PipelineOverview', () => {
  it('renders pipeline stages with counts', () => {
    const { container } = render(PipelineOverview, {
      props: {
        specs: { total: 3, pending: 1, approved: 2 },
        tasks: { total: 7, in_progress: 2, blocked: 1, done: 4 },
        agents: { total: 5, active: 2 },
        mrs: { total: 4, open: 2, merged: 1, failed_gates: 1 },
      },
    });

    const overview = container.querySelector('[data-testid="pipeline-overview"]');
    expect(overview).toBeTruthy();
    expect(overview.textContent).toContain('Specs');
    expect(overview.textContent).toContain('Tasks');
    expect(overview.textContent).toContain('Agents');
    expect(overview.textContent).toContain('MRs');
    expect(overview.textContent).toContain('Merged');
  });

  it('calls onStageClick when a pipeline stage is clicked', async () => {
    const onStageClick = vi.fn();
    const { container } = render(PipelineOverview, {
      props: {
        specs: { total: 3, pending: 1, approved: 2 },
        onStageClick,
      },
    });

    const stages = container.querySelectorAll('.pipeline-stage');
    expect(stages.length).toBeGreaterThan(0);
    await fireEvent.click(stages[0]);
    expect(onStageClick).toHaveBeenCalledWith('specs');
  });
});
