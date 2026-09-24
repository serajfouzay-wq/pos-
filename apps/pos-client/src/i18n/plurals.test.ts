import { describe, expect, it } from 'vitest';
import { fillPluralForms } from './plurals';

describe('plural forms', () => {
  it('fills the forms a language has from its own _other text', () => {
    const ar = fillPluralForms(
      { orders: { seats_one: 'مقعد واحد', seats_other: '{{count}} مقاعد' } },
      'ar',
    );
    expect(ar.orders).toEqual({
      seats_one: 'مقعد واحد',
      seats_other: '{{count}} مقاعد',
      seats_zero: '{{count}} مقاعد',
      seats_two: '{{count}} مقاعد',
      seats_few: '{{count}} مقاعد',
      seats_many: '{{count}} مقاعد',
    });
  });

  it('leaves English as it is', () => {
    const en = { a_one: 'one', a_other: 'many', b: 'plain' };
    expect(fillPluralForms(en, 'en')).toEqual(en);
  });
});
