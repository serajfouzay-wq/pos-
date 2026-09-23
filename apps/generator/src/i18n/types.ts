import type { en } from './locales/en';

type DeepString<T> = { readonly [K in keyof T]: T[K] extends string ? string : DeepString<T[K]> };

/** Every locale must provide exactly the keys of the English source. */
export type Translation = DeepString<typeof en>;
