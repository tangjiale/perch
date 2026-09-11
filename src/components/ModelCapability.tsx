import { useId } from "react";
import type { Model } from "../lib/types";
import "./model-capability.css";

const choices = [
  { value: "chat", label: "文本" },
  { value: "vision", label: "视觉" },
  { value: "embedding", label: "向量" },
] as const;

export default function ModelCapability({
  value,
  onValueChange,
  disabled = false,
  label = "模型类型",
}: {
  value: Model["capability"];
  onValueChange: (value: Model["capability"]) => void;
  disabled?: boolean;
  label?: string;
}) {
  const name = useId();
  return (
    <fieldset
      className="model-capability"
      disabled={disabled}
      aria-label={label}
    >
      {choices.map((choice) => (
        <label key={choice.value}>
          <input
            type="radio"
            name={name}
            value={choice.value}
            checked={value === choice.value}
            onChange={() => onValueChange(choice.value)}
          />
          <span>{choice.label}</span>
        </label>
      ))}
    </fieldset>
  );
}
