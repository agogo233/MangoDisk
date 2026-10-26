import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), stop: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ listen: mocks.listen }) }));
import { ExplorerIntegrationService } from './explorer-integration-service';

beforeEach(() => {
  vi.clearAllMocks();
  mocks.listen.mockResolvedValue(mocks.stop);
  mocks.invoke.mockResolvedValue(null);
});

describe('Explorer directory handoff', () => {
  it('subscribes before reading a cold-start request and releases the listener', async () => {
    const handler = vi.fn();
    mocks.invoke.mockResolvedValueOnce({ schemaVersion: 1, path: 'C:\\中文 & folders' });
    const stop = await ExplorerIntegrationService.watchAnalysisLaunch(handler);
    expect(mocks.listen.mock.invocationCallOrder[0]).toBeLessThan(mocks.invoke.mock.invocationCallOrder[0]);
    expect(handler).toHaveBeenCalledExactlyOnceWith('C:\\中文 & folders');
    stop();
    expect(mocks.stop).toHaveBeenCalledOnce();
  });

  it('reads the pending request on a warm notification and rejects unknown schemas', async () => {
    const handler = vi.fn();
    await ExplorerIntegrationService.watchAnalysisLaunch(handler);
    const notify = mocks.listen.mock.calls[0][1] as () => void;
    mocks.invoke.mockResolvedValueOnce({ schemaVersion: 2, path: 'C:\\ignored' });
    notify();
    await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalledTimes(2));
    expect(handler).not.toHaveBeenCalled();
    mocks.invoke.mockResolvedValueOnce({ schemaVersion: 1, path: 'C:\\selected' });
    notify();
    await vi.waitFor(() => expect(handler).toHaveBeenCalledExactlyOnceWith('C:\\selected'));
  });

  it('releases its native listener when the initial handoff fails', async () => {
    mocks.invoke.mockRejectedValueOnce(new Error('transport failed'));
    await expect(ExplorerIntegrationService.watchAnalysisLaunch(vi.fn())).rejects.toThrow('transport failed');
    expect(mocks.stop).toHaveBeenCalledOnce();
  });
});
