import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render } from '@testing-library/svelte';
import Explorer from '../components/Explorer.svelte';

// Mock the api module so tests don't make real HTTP requests
vi.mock('../lib/api.js', () => ({
  api: {
    allRepos: vi.fn().mockResolvedValue([
      { id: 'repo-1', name: 'test-repo' },
    ]),
    graphNodes: vi.fn().mockResolvedValue([
      {
        id: 'node-1',
        node_type: 'Module',
        qualified_name: 'crate::domain::task',
        file_path: 'src/domain/task.rs',
        visibility: 'pub',
        spec_path: 'specs/system/platform-model.md',
        spec_confidence: 0.9,
        complexity: 5,
        churn_count_30d: 2,
      },
      {
        id: 'node-2',
        node_type: 'Trait',
        qualified_name: 'crate::ports::TaskPort',
        file_path: 'src/ports/mod.rs',
        visibility: 'pub',
      },
    ]),
    graphEdges: vi.fn().mockResolvedValue([
      { id: 'edge-1', source_id: 'node-1', target_id: 'node-2', edge_type: 'implements' },
    ]),
  },
  setAuthToken: vi.fn(),
}));

// Mock D3 force simulation (jsdom has no requestAnimationFrame timers)
vi.mock('d3', () => ({
  forceSimulation: vi.fn(() => ({
    force: vi.fn().mockReturnThis(),
    alphaDecay: vi.fn().mockReturnThis(),
    on: vi.fn().mockReturnThis(),
    stop: vi.fn(),
  })),
  forceLink: vi.fn(() => ({
    id: vi.fn().mockReturnThis(),
    distance: vi.fn().mockReturnThis(),
    strength: vi.fn().mockReturnThis(),
  })),
  forceManyBody: vi.fn(() => ({ strength: vi.fn().mockReturnThis() })),
  forceCenter: vi.fn(),
  forceCollide: vi.fn(),
}));

// Provide the navigate context expected by Explorer
import { setContext } from 'svelte';

describe('Explorer', () => {
  it('renders without throwing', () => {
    expect(() => render(Explorer)).not.toThrow();
  });

  it('mounts and produces DOM output', () => {
    const { container } = render(Explorer);
    expect(container).toBeTruthy();
    expect(container.innerHTML.length).toBeGreaterThan(0);
  });

  it('shows controls bar with repo selector', () => {
    const { container } = render(Explorer);
    const select = container.querySelector('#repo-select');
    expect(select).toBeTruthy();
  });

  it('shows name filter input', () => {
    const { container } = render(Explorer);
    const input = container.querySelector('#name-filter');
    expect(input).toBeTruthy();
  });

  it('shows risk map toggle', () => {
    const { container } = render(Explorer);
    const label = container.querySelector('.risk-toggle');
    expect(label).toBeTruthy();
  });

  it('shows node type filter pills', () => {
    const { container } = render(Explorer);
    const pills = container.querySelectorAll('.type-pill');
    expect(pills.length).toBeGreaterThan(0);
  });

  it('shows node count display', () => {
    const { container } = render(Explorer);
    const count = container.querySelector('.node-count');
    expect(count).toBeTruthy();
  });
});
