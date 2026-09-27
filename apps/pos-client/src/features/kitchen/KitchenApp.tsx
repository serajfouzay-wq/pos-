import type { KitchenTicket } from '@pos/shared';
import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  useAppInfo,
  useBumpTicket,
  useKitchenBoard,
  useStrikeItem,
  useSyncEvents,
} from '../../ipc/queries';
import { toggleFullscreen } from '../../ipc/window';
import { formatTime } from '../../lib/dates';
import { formatQuantity } from '../../lib/money';
import { useUiStore } from '../../stores/ui';
import { allDay, arrived, minutesOpen, urgency } from './board';

/** A short two-tone chime for a new ticket (no audio files to ship). */
function chime() {
  try {
    const audio = new AudioContext();
    [880, 1320].forEach((frequency, i) => {
      const osc = audio.createOscillator();
      const gain = audio.createGain();
      osc.frequency.value = frequency;
      gain.gain.setValueAtTime(0.15, audio.currentTime + i * 0.18);
      gain.gain.exponentialRampToValueAtTime(0.001, audio.currentTime + i * 0.18 + 0.25);
      osc.connect(gain).connect(audio.destination);
      osc.start(audio.currentTime + i * 0.18);
      osc.stop(audio.currentTime + i * 0.18 + 0.3);
    });
  } catch {
    // No audio device: the ticket still appears.
  }
}

/** Re-renders every `ms` so the ticket timers move. */
function useNow(ms: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => {
      setNow(Date.now());
    }, ms);
    return () => {
      clearInterval(id);
    };
  }, [ms]);
  return now;
}

function TicketCard({ ticket, now }: { ticket: KitchenTicket; now: number }) {
  const { t } = useTranslation();
  const bump = useBumpTicket();
  const strike = useStrikeItem();
  const minutes = minutesOpen(ticket, now);
  const tone = ticket.kind === 'void' ? 'void' : urgency(minutes);
  const allDone = ticket.items.every((i) => i.done_at !== null);

  return (
    <motion.li
      layout
      className={`ticket ticket--${tone}`}
      initial={{ opacity: 0, scale: 0.9, y: 20 }}
      animate={{ opacity: 1, scale: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.8, y: -40, transition: { duration: 0.25 } }}
      transition={{ type: 'spring', stiffness: 380, damping: 30 }}
    >
      <header className="ticket__head">
        <span className="ticket__number">#{ticket.ticket_number}</span>
        <span className="ticket__title">
          {ticket.kind === 'void' && <strong className="ticket__void">{t('kitchen.void')} </strong>}
          {ticket.title}
        </span>
        <span className="ticket__timer">{t('kitchen.minutes', { n: minutes })}</span>
      </header>
      <p className="ticket__meta">
        {t(`kitchen.orderType.${ticket.order_type}`)}
        {ticket.course !== null && ` · ${t('kitchen.course', { n: ticket.course })}`}
        {ticket.guests > 0 && ` · ${t('kitchen.guests', { count: ticket.guests })}`} ·{' '}
        {ticket.server_name}
      </p>
      <ul className="ticket__items">
        {ticket.items.map((item) => (
          <li key={item.line_id}>
            <button
              type="button"
              className={item.done_at ? 'ticket__item ticket__item--done' : 'ticket__item'}
              disabled={ticket.kind === 'void' || strike.isPending}
              onClick={() => {
                strike.mutate({
                  ticketId: ticket.id,
                  lineId: item.line_id,
                  done: item.done_at === null,
                });
              }}
            >
              <span className="ticket__qty">{formatQuantity(item.quantity_milli)}×</span>
              <span className="ticket__name">
                {item.name}
                {item.modifiers.map((m) => (
                  <span key={m} className="ticket__modifier">
                    + {m}
                  </span>
                ))}
                {item.note && <span className="ticket__note">! {item.note}</span>}
              </span>
            </button>
          </li>
        ))}
      </ul>
      <button
        type="button"
        className={
          allDone || ticket.kind === 'void' ? 'ticket__bump ticket__bump--go' : 'ticket__bump'
        }
        disabled={bump.isPending}
        onClick={() => {
          bump.mutate({ ticketId: ticket.id, ready: true });
        }}
      >
        {ticket.kind === 'void' ? '✓' : t('kitchen.ready')}
      </button>
    </motion.li>
  );
}

/**
 * The kitchen display window (`?window=kds`). No sign-in: the window can
 * only read the board, strike items and bump tickets (its capability).
 */
export function KitchenApp() {
  const { t } = useTranslation();
  const info = useAppInfo();
  const board = useKitchenBoard(30);
  const recall = useBumpTicket();
  const setLocale = useUiStore((s) => s.setLocale);
  const locale = useUiStore((s) => s.locale);
  const now = useNow(15_000);
  const [sound, setSound] = useState(true);
  const [showAllDay, setShowAllDay] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const seen = useRef<Set<string> | null>(null);
  useSyncEvents();

  const defaultLocale = info.data?.client.locale.default;
  useEffect(() => {
    if (defaultLocale) setLocale(defaultLocale);
  }, [defaultLocale, setLocale]);

  const open = board.data?.open ?? [];
  const ready = board.data?.ready ?? [];
  useEffect(() => {
    if (!board.data) return;
    if (sound && arrived(seen.current, board.data.open).length > 0) chime();
    seen.current = new Set(board.data.open.map((ticket) => ticket.id));
  }, [board.data, sound]);

  return (
    <div className="kitchen">
      <header className="kitchen__bar">
        <strong>
          {info.data?.client.display_name} · {t('kitchen.title')}
        </strong>
        <span className="kitchen__count">{t('kitchen.open', { count: open.length })}</span>
        <span className="grow" />
        <button
          type="button"
          className="chip"
          aria-pressed={showAllDay}
          onClick={() => {
            setShowAllDay(!showAllDay);
          }}
        >
          {t('kitchen.allDay')}
        </button>
        <button
          type="button"
          className="chip"
          aria-pressed={sound}
          onClick={() => {
            setSound(!sound);
          }}
        >
          {t('kitchen.sound')}
        </button>
        <button
          type="button"
          className="chip"
          onClick={() => {
            void toggleFullscreen().then(setFullscreen);
          }}
        >
          {fullscreen ? t('kitchen.exitFullScreen') : t('kitchen.fullScreen')}
        </button>
        <span className="kitchen__clock">{formatTime(new Date(now).toISOString(), locale)}</span>
      </header>
      {board.error && (
        <p role="alert" className="banner banner--warning">
          {board.error.message}
        </p>
      )}
      <div className="kitchen__main">
        {open.length === 0 && board.data ? (
          <motion.p className="kitchen__empty" initial={{ opacity: 0 }} animate={{ opacity: 1 }}>
            {t('kitchen.empty')}
          </motion.p>
        ) : (
          <ul className="kitchen__tickets">
            <AnimatePresence initial={false}>
              {open.map((ticket) => (
                <TicketCard key={ticket.id} ticket={ticket} now={now} />
              ))}
            </AnimatePresence>
          </ul>
        )}
        <AnimatePresence>
          {showAllDay && (
            <motion.aside
              className="kitchen__allday"
              initial={{ x: 40, opacity: 0 }}
              animate={{ x: 0, opacity: 1 }}
              exit={{ x: 40, opacity: 0 }}
            >
              <h2>{t('kitchen.allDay')}</h2>
              <ul>
                {allDay(open).map((line) => (
                  <li key={line.key}>
                    <strong>{formatQuantity(line.quantity_milli)}×</strong> {line.name}
                    {line.modifiers.length > 0 && (
                      <span className="muted small"> + {line.modifiers.join(', ')}</span>
                    )}
                  </li>
                ))}
              </ul>
            </motion.aside>
          )}
        </AnimatePresence>
      </div>
      <footer className="kitchen__recall">
        <span className="muted small">{t('kitchen.recent')}</span>
        {ready.length === 0 && <span className="muted small">{t('kitchen.noRecent')}</span>}
        <AnimatePresence initial={false}>
          {ready.map((ticket) => (
            <motion.button
              key={ticket.id}
              layout
              type="button"
              className="chip"
              initial={{ opacity: 0, y: 10 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0 }}
              disabled={recall.isPending}
              title={t('kitchen.recall')}
              onClick={() => {
                recall.mutate({ ticketId: ticket.id, ready: false });
              }}
            >
              ↺ #{ticket.ticket_number} {ticket.title}
            </motion.button>
          ))}
        </AnimatePresence>
      </footer>
    </div>
  );
}
