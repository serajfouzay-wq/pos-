import type { CommandArgs, CommandName, CommandResult, IpcContract } from './contract';
import { IpcError } from './errors';

/** Structural match for `@tauri-apps/api/core#invoke`, injected for testability. */
export type InvokeFn = (command: string, args: Record<string, unknown>) => Promise<unknown>;

export interface IpcClientOptions {
  /**
   * Validate results against the contract's Zod schema. Arguments are always
   * validated. Defaults to `true`; result validation catches Rust/TS drift early.
   */
  readonly validateResults?: boolean;
}

type ArgsTuple<C extends IpcContract, K extends CommandName<C>> =
  Record<string, never> extends CommandArgs<C, K>
    ? [args?: CommandArgs<C, K>]
    : [args: CommandArgs<C, K>];

export interface IpcClient<C extends IpcContract> {
  call<K extends CommandName<C>>(
    command: K,
    ...args: ArgsTuple<C, K>
  ): Promise<CommandResult<C, K>>;
}

/**
 * The ONLY path from the frontend to SQLite, hardware and the cloud.
 * Errors always surface as {@link IpcError}.
 */
export function createIpcClient<C extends IpcContract>(
  contract: C,
  invoke: InvokeFn,
  options: IpcClientOptions = {},
): IpcClient<C> {
  const validateResults = options.validateResults ?? true;

  return {
    async call<K extends CommandName<C>>(
      name: K,
      ...[args]: ArgsTuple<C, K>
    ): Promise<CommandResult<C, K>> {
      const def = contract[name];
      if (!def) throw new IpcError('internal', `unknown IPC command "${name}"`, name);

      const parsedArgs = def.args.safeParse(args ?? {});
      if (!parsedArgs.success) {
        throw new IpcError('validation', parsedArgs.error.message, name, {
          cause: parsedArgs.error,
        });
      }

      let raw: unknown;
      try {
        raw = await invoke(name, parsedArgs.data);
      } catch (error) {
        throw IpcError.from(name, error);
      }

      if (!validateResults) return raw as CommandResult<C, K>;
      const parsedResult = def.result.safeParse(raw);
      if (!parsedResult.success) {
        throw new IpcError(
          'internal',
          `"${name}" returned a value that violates the IPC contract: ${parsedResult.error.message}`,
          name,
          { cause: parsedResult.error },
        );
      }
      return parsedResult.data as CommandResult<C, K>;
    },
  };
}
