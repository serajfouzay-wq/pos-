import type { Translation } from '../types';

export const ar: Translation = {
  app: {
    title: 'نقطة البيع',
    loading: 'جارٍ التشغيل…',
  },
  shell: {
    ready: 'الواجهة جاهزة',
    phase: 'المرحلة ١ · الأساس',
    businessType: {
      retail: 'تجزئة',
      cafe: 'مقهى',
      restaurant: 'مطعم',
    },
    baseCurrency: 'العملة الأساسية',
    version: 'الإصدار {{version}} ({{profile}})',
    language: 'اللغة',
    notInTauri: 'يجب تشغيل هذه الشاشة داخل تطبيق نقطة البيع.',
    error: 'تعذّر الوصول إلى نواة نقطة البيع: {{message}}',
  },
};
