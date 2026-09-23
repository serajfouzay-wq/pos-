import type { Translation } from '../types';

export const ar: Translation = {
  app: {
    title: 'نقطة البيع',
    loading: 'جارٍ التشغيل…',
  },
  shell: {
    ready: 'الواجهة جاهزة',
    phase: 'المرحلة ٢ · الترخيص والتخزين الآمن',
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
  license: {
    checking: 'جارٍ التحقق من الترخيص…',
    devKeyBanner:
      'مفتاح ترخيص تجريبي — هذا الإصدار يقبل تراخيص يمكن لأي شخص إنشاؤها. ليس للعملاء الفعليين.',
    graceBanner_one: 'غير متصل: اتصل بالإنترنت خلال يوم واحد لمواصلة البيع.',
    graceBanner_other: 'غير متصل: اتصل بالإنترنت خلال {{count}} أيام لمواصلة البيع.',
    activation: {
      step1: '١ · أرسل رمز التفعيل هذا إلى مزوّد نظام نقاط البيع',
      device: 'هذا الجهاز: {{name}}',
      copy: 'نسخ الرمز',
      copied: 'تم النسخ ✓',
      step2: '٢ · الصق الترخيص الذي استلمته',
      tokenPlaceholder: 'eyJhbGciOiJSUzI1NiIs…',
      submit: 'تفعيل',
      activating: 'جارٍ التفعيل…',
    },
    state: {
      missing: { title: 'فعّل هذا الجهاز', body: 'يحتاج نظام نقاط البيع إلى ترخيص قبل استخدامه.' },
      invalid_token: { title: 'الترخيص غير مقبول', body: 'الترخيص المثبّت غير صالح لهذا النظام.' },
      fingerprint_mismatch: {
        title: 'تم اكتشاف جهاز مختلف',
        body: 'الترخيص يخص جهازًا آخر، أو تغيّرت مكوّنات هذا الجهاز. اطلب ترخيصًا جديدًا.',
      },
      expired: { title: 'انتهت صلاحية الترخيص', body: 'جدّد الترخيص لدى مزوّد النظام.' },
      revoked: { title: 'تم إلغاء الترخيص', body: 'قام مزوّد النظام بإيقاف هذا الجهاز.' },
      grace_exhausted: {
        title: 'غير متصل لفترة طويلة',
        body: 'صِل هذا الجهاز بالإنترنت لتأكيد الترخيص.',
      },
      hardware_error: {
        title: 'فشل فحص الجهاز',
        body: 'تعذّرت قراءة هوية الجهاز. أعد تشغيله، وتواصل مع الدعم إن استمرت المشكلة.',
      },
      storage_error: {
        title: 'مشكلة في التخزين',
        body: 'تعذّر فتح البيانات المحلية. تواصل مع الدعم.',
      },
    },
  },
};
