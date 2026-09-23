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
    phase: 'المرحلة ٢ · الترخيص والتخزين الآمن',
    comingSoon: 'تتوفر مساحة العمل هذه في المرحلة ٥.',
    version: 'الإصدار {{version}} ({{profile}})',
    language: 'اللغة',
    notInTauri: 'يجب تشغيل هذه الشاشة داخل تطبيق مصنع نقاط البيع.',
    error: 'تعذّر الوصول إلى نواة المولّد: {{message}}',
  },
  licenses: {
    copy: 'نسخ',
    copied: 'تم النسخ ✓',
    working: 'جارٍ التنفيذ…',
    businessType: { retail: 'تجزئة', cafe: 'مقهى', restaurant: 'مطعم' },
    key: {
      title: 'مفتاح التوقيع',
      state: {
        absent: 'لا يوجد مفتاح توقيع بعد. أنشئ واحدًا لبدء إصدار التراخيص.',
        locked: 'مفتاح التوقيع مقفل. أدخل عبارة المرور لإصدار التراخيص.',
        unlocked: 'مفتوح لهذه الجلسة. أقفله عند الانتهاء.',
      },
      backupWarning:
        'احتفظ بنسخة احتياطية من ملف المفتاح وتذكّر عبارة المرور. إن فُقد أيٌّ منهما يجب إعادة بناء جميع إصدارات العملاء بمفتاح جديد.',
      passphrase: 'عبارة المرور',
      confirm: 'تأكيد عبارة المرور',
      minLength: '{{count}} حرفًا على الأقل.',
      create: 'إنشاء مفتاح التوقيع',
      unlock: 'فتح',
      lock: 'قفل المفتاح',
      keyId: 'معرّف المفتاح:',
      publicKeyHelp:
        'المفتاح العام — استخدمه في POS_LICENSE_PUBLIC_KEY عند بناء إصدارات العملاء، واحفظه كسرّ LICENSE_PUBLIC_KEY_PEM لوظيفة license-validate.',
    },
    issue: {
      title: 'إصدار ترخيص جهاز',
      unlockFirst: 'افتح مفتاح التوقيع أولًا.',
      code: 'رمز التفعيل من الجهاز',
      device: 'الجهاز',
      client: 'معرّف العميل',
      fingerprint: 'البصمة',
      slug: 'المعرّف المختصر للعميل',
      businessType: 'نوع النشاط',
      maxDevices: 'الحد الأقصى للأجهزة (للعميل كاملًا)',
      expiry: 'تاريخ الانتهاء (اختياري)',
      submit: 'توقيع الترخيص',
      result: 'أرسل هذا الترخيص إلى الجهاز والصقه في شاشة التفعيل.',
    },
  },
};
