import type { Product } from '@pos/shared';
import { useTranslation } from 'react-i18next';
import { errorText } from '../../components/Toast';
import { ipc } from '../../ipc';
import { useBarcodeScanner } from '../../lib/useBarcodeScanner';

/**
 * Barcode → product → `pick`. Used by the global scanner and by Enter in the
 * retail search box; resolves whether a product was found.
 */
export function useBarcodeLookup(
  pick: (product: Product) => void,
  toast: (message: string) => void,
  scannerEnabled: boolean,
): (code: string) => Promise<boolean> {
  const { t } = useTranslation();
  const lookup = async (code: string) => {
    try {
      const [product] = await ipc.call('get_products', { filter: { barcode: code, limit: 1 } });
      if (product) {
        pick(product);
        return true;
      }
      toast(t('sell.unknownBarcode', { code }));
    } catch (e) {
      toast(errorText(e));
    }
    return false;
  };
  useBarcodeScanner((code) => {
    void lookup(code);
  }, scannerEnabled);
  return lookup;
}
