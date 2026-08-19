"use client";

import { useState, useRef, useEffect, useCallback } from "react";
import { Calendar, ChevronLeft, ChevronRight, Clock } from "lucide-react";
import { useI18n } from "@/app/i18n/I18nContext";

interface DateTimePickerProps {
  value: Date;
  onChange: (date: Date) => void;
}


function getDaysInMonth(year: number, month: number) {
  return new Date(year, month + 1, 0).getDate();
}

function getFirstDayOfMonth(year: number, month: number) {
  return new Date(year, month, 1).getDay();
}

function pad(n: number) {
  return String(n).padStart(2, "0");
}

function formatDisplay(date: Date): string {
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function isSameDay(a: Date, b: Date) {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

export default function DateTimePicker({ value, onChange }: DateTimePickerProps) {
  const { t } = useI18n();
  const DAYS = t.datePicker.days;
  const MONTHS = t.datePicker.months;
  const [open, setOpen] = useState(false);
  const [viewYear, setViewYear] = useState(value.getFullYear());
  const [viewMonth, setViewMonth] = useState(value.getMonth());
  const [hours, setHours] = useState(pad(value.getHours()));
  const [minutes, setMinutes] = useState(pad(value.getMinutes()));
  const containerRef = useRef<HTMLDivElement>(null);

  // Sync view when value changes externally
  useEffect(() => {
    setViewYear(value.getFullYear());
    setViewMonth(value.getMonth());
    setHours(pad(value.getHours()));
    setMinutes(pad(value.getMinutes()));
  }, [value]);

  // Close on outside click
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [open]);

  // Close on Escape key
  useEffect(() => {
    if (!open) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setOpen(false);
      }
    };
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [open]);

  const prevMonth = () => {
    if (viewMonth === 0) {
      setViewMonth(11);
      setViewYear((y) => y - 1);
    } else {
      setViewMonth((m) => m - 1);
    }
  };

  const nextMonth = () => {
    if (viewMonth === 11) {
      setViewMonth(0);
      setViewYear((y) => y + 1);
    } else {
      setViewMonth((m) => m + 1);
    }
  };

  const selectDay = useCallback(
    (day: number) => {
      const h = parseInt(hours, 10) || 0;
      const m = parseInt(minutes, 10) || 0;
      const newDate = new Date(viewYear, viewMonth, day, h, m);
      onChange(newDate);
    },
    [viewYear, viewMonth, hours, minutes, onChange]
  );

  const applyTime = useCallback(() => {
    const h = Math.min(23, Math.max(0, parseInt(hours, 10) || 0));
    const m = Math.min(59, Math.max(0, parseInt(minutes, 10) || 0));
    const newDate = new Date(value);
    newDate.setHours(h, m, 0, 0);
    setHours(pad(h));
    setMinutes(pad(m));
    onChange(newDate);
  }, [value, hours, minutes, onChange]);

  const goToToday = () => {
    const now = new Date();
    setViewYear(now.getFullYear());
    setViewMonth(now.getMonth());
  };

  // Build calendar grid
  const daysInMonth = getDaysInMonth(viewYear, viewMonth);
  const firstDay = getFirstDayOfMonth(viewYear, viewMonth);
  const today = new Date();

  const calendarCells: (number | null)[] = [];
  for (let i = 0; i < firstDay; i++) calendarCells.push(null);
  for (let d = 1; d <= daysInMonth; d++) calendarCells.push(d);

  return (
    <div ref={containerRef} className="dtp">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="date-input dtp__trigger"
        aria-expanded={open}
      >
        <Calendar size={13} aria-hidden="true" />
        <span className="mono">{formatDisplay(value)}</span>
      </button>

      {open && (
        <div className="dtp__panel">
          <div className="dtp__head">
            <button type="button" onClick={prevMonth} className="dtp__nav" aria-label={t.datePicker.prevMonth}>
              <ChevronLeft size={15} aria-hidden="true" />
            </button>
            <button type="button" onClick={goToToday} className="dtp__month">
              {t.datePicker.monthYearTemplate
                .replace("{year}", String(viewYear))
                .replace("{month}", MONTHS[viewMonth])}
            </button>
            <button type="button" onClick={nextMonth} className="dtp__nav" aria-label={t.datePicker.nextMonth}>
              <ChevronRight size={15} aria-hidden="true" />
            </button>
          </div>

          <div className="dtp__dow">
            {DAYS.map((d) => (
              <span key={d}>{d}</span>
            ))}
          </div>

          <div className="dtp__grid">
            {calendarCells.map((day, i) => {
              if (day === null) {
                return <div key={`empty-${i}`} />;
              }
              const cellDate = new Date(viewYear, viewMonth, day);
              const isSelected = isSameDay(cellDate, value);
              const isToday = isSameDay(cellDate, today);

              return (
                <button
                  key={day}
                  type="button"
                  onClick={() => selectDay(day)}
                  aria-current={isToday ? "date" : undefined}
                  aria-pressed={isSelected}
                  className={`calendar-day${isSelected ? " calendar-day-selected" : ""}`}
                >
                  {day}
                </button>
              );
            })}
          </div>

          <div className="dtp__time">
            <Clock size={13} aria-hidden="true" />
            <input
              type="text"
              value={hours}
              onChange={(e) => setHours(e.target.value.replace(/\D/g, "").slice(0, 2))}
              onBlur={applyTime}
              className="dtp__time-input"
              maxLength={2}
              placeholder="HH"
              aria-label={t.datePicker.hours}
            />
            <span className="dtp__time-sep">:</span>
            <input
              type="text"
              value={minutes}
              onChange={(e) => setMinutes(e.target.value.replace(/\D/g, "").slice(0, 2))}
              onBlur={applyTime}
              onKeyDown={(e) => { if (e.key === "Enter") applyTime(); }}
              className="dtp__time-input"
              maxLength={2}
              placeholder="MM"
              aria-label={t.datePicker.minutes}
            />
            <button
              type="button"
              className="btn btn--secondary btn--sm dtp__now"
              onClick={() => {
                const now = new Date();
                const newDate = new Date(value);
                newDate.setHours(now.getHours(), now.getMinutes(), 0, 0);
                setHours(pad(now.getHours()));
                setMinutes(pad(now.getMinutes()));
                onChange(newDate);
              }}
            >
              {t.datePicker.now}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
