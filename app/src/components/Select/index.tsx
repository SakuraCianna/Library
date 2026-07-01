import { useState, useRef, useEffect } from "react";
import { Icon } from "@iconify/react";
import chevronDown from "@iconify-icons/lucide/chevron-down";
import checkIcon from "@iconify-icons/lucide/check";
import styles from "./Select.module.css";

export interface SelectOption {
  value: string;
  label: string;
}

export interface SelectProps {
  options: SelectOption[];
  value: string;
  onChange: (value: string) => void;
  className?: string;
  "aria-label"?: string;
  disabled?: boolean;
}

export function Select({
  options,
  value,
  onChange,
  className,
  "aria-label": ariaLabel,
  disabled,
}: SelectProps) {
  const [isOpen, setIsOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const selectedOption = options.find((opt) => opt.value === value) || options[0];

  useEffect(() => {
    const handleOutsideClick = (e: MouseEvent) => {
      if (
        containerRef.current &&
        !containerRef.current.contains(e.target as Node)
      ) {
        setIsOpen(false);
      }
    };
    if (isOpen) {
      document.addEventListener("mousedown", handleOutsideClick);
    }
    return () => {
      document.removeEventListener("mousedown", handleOutsideClick);
    };
  }, [isOpen]);

  const handleSelect = (val: string) => {
    onChange(val);
    setIsOpen(false);
  };

  return (
    <div
      className={`${styles.selectContainer} ${className || ""}`}
      ref={containerRef}
    >
      <button
        type="button"
        className={`${styles.selectTrigger} ${disabled ? styles.disabled : ""} ${isOpen ? styles.open : ""}`}
        aria-label={ariaLabel}
        disabled={disabled}
        onClick={() => setIsOpen(!isOpen)}
      >
        <span className={styles.selectValue}>{selectedOption?.label}</span>
        <Icon icon={chevronDown} className={styles.chevron} />
      </button>

      {isOpen && !disabled && (
        <ul className={styles.optionsList}>
          {options.map((opt) => (
            <li
              key={opt.value}
              className={`${styles.optionItem} ${opt.value === value ? styles.selected : ""}`}
              onClick={() => handleSelect(opt.value)}
            >
              <span className={styles.optionLabel}>{opt.label}</span>
              {opt.value === value && (
                <Icon icon={checkIcon} className={styles.checkIcon} />
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
