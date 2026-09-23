import type { Translation } from '../types';

export const ar: Translation = {
  app: {
    title: 'مصنع نقاط البيع',
    loading: 'جارٍ التشغيل…',
  },
  nav: {
    clients: 'العملاء',
    builds: 'الإصدارات',
    licenses: 'التراخيص',
    settings: 'الإعدادات',
  },
  shell: {
    phase: 'المرحلة ١ · الأساس',
    comingSoon: 'تتوفر مساحة العمل هذه في المرحلة ٥.',
    version: 'الإصدار {{version}} ({{profile}})',
    language: 'اللغة',
    notInTauri: 'يجب تشغيل هذه الشاشة داخل تطبيق مصنع نقاط البيع.',
    error: 'تعذّر الوصول إلى نواة المولّد: {{message}}',
  },
};
