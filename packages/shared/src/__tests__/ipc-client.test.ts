import { describe, expect, it, vi } from 'vitest';
import { createIpcClient, type InvokeFn } from '../ipc/client';
import { IpcError } from '../ipc/errors';
import { POS_IPC } from '../ipc/pos-contract';

const uuid = '00000000-0000-4000-8000-000000000001';

describe('createIpcClient', () => {
  it('forwards validated snake_case args and returns the parsed result', async () => {
    const invoke = vi.fn<InvokeFn>().mockResolvedValue(null);
    const ipc = createIpcClient(POS_IPC, invoke);
    await expect(ipc.call('print_receipt', { transaction_id: uuid })).resolves.toBeNull();
    expect(invoke).toHaveBeenCalledWith('print_receipt', { transaction_id: uuid });
  });

  it('applies schema defaults before invoking', async () => {
    const invoke = vi.fn<InvokeFn>().mockResolvedValue([]);
    const ipc = createIpcClient(POS_IPC, invoke);
    await ipc.call('get_products', { filter: { search: 'latte' } });
    expect(invoke).toHaveBeenCalledWith('get_products', {
      filter: { search: 'latte', limit: 200, offset: 0 },
    });
  });

  it('rejects invalid args without reaching Rust', async () => {
    const invoke = vi.fn<InvokeFn>();
    const ipc = createIpcClient(POS_IPC, invoke);
    await expect(ipc.call('print_receipt', { transaction_id: 'nope' })).rejects.toMatchObject({
      code: 'validation',
    });
    expect(invoke).not.toHaveBeenCalled();
  });

  it('maps Rust errors to typed IpcError', async () => {
    const invoke = vi.fn<InvokeFn>().mockRejectedValue({ code: 'forbidden', message: 'no' });
    const ipc = createIpcClient(POS_IPC, invoke);
    const error: unknown = await ipc.call('kick_cash_drawer').catch((e: unknown) => e);
    expect(error).toBeInstanceOf(IpcError);
    expect(error).toMatchObject({ code: 'forbidden', command: 'kick_cash_drawer' });
  });

  it('flags results that violate the contract', async () => {
    const invoke = vi.fn<InvokeFn>().mockResolvedValue({ state: 'bogus' });
    const ipc = createIpcClient(POS_IPC, invoke);
    await expect(ipc.call('verify_license')).rejects.toMatchObject({ code: 'internal' });
  });
});
