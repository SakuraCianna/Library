import React, { useEffect, useState } from "react";
import { getUserSettings, updateUserSettings } from "../lib/tauriClient";

interface SettingsModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSaved: () => void;
}

export const SettingsModal: React.FC<SettingsModalProps> = ({
  isOpen,
  onClose,
  onSaved,
}) => {
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("deepseek-v4-flash");
  const [baseUrl, setBaseUrl] = useState("https://api.deepseek.com");
  const [isLoading, setIsLoading] = useState(false);

  useEffect(() => {
    if (isOpen) {
      loadSettings();
    }
  }, [isOpen]);

  const loadSettings = async () => {
    try {
      const settings = await getUserSettings();
      setApiKey(settings.deepseek_api_key || "");
      setModel(settings.deepseek_model || "deepseek-v4-flash");
      setBaseUrl(settings.deepseek_base_url || "https://api.deepseek.com");
    } catch (e) {
      console.error("Failed to load settings:", e);
    }
  };

  const handleSave = async () => {
    setIsLoading(true);
    try {
      await updateUserSettings({
        deepseek_api_key: apiKey,
        deepseek_model: model,
        deepseek_base_url: baseUrl,
      });
      onSaved();
    } catch (e) {
      console.error("Failed to save settings:", e);
      alert("保存失败：" + String(e));
    } finally {
      setIsLoading(false);
    }
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
      <div className="w-[480px] bg-[#1E1E1E] border border-white/10 rounded-xl shadow-2xl p-6 flex flex-col space-y-6">
        <div className="flex items-center justify-between">
          <h2 className="text-xl font-semibold text-white">模型 API 设置</h2>
          <button
            onClick={onClose}
            className="text-white/50 hover:text-white transition-colors"
          >
            ✕
          </button>
        </div>

        <div className="flex flex-col space-y-4">
          <div className="flex flex-col space-y-1.5">
            <label className="text-sm font-medium text-white/80">
              DeepSeek API Key
            </label>
            <input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              className="px-3 py-2 bg-black/20 border border-white/10 rounded-lg text-white focus:outline-none focus:border-blue-500 transition-colors"
              placeholder="sk-..."
            />
          </div>

          <div className="flex flex-col space-y-1.5">
            <label className="text-sm font-medium text-white/80">
              选择模型
            </label>
            <select
              value={model}
              onChange={(e) => setModel(e.target.value)}
              className="px-3 py-2 bg-black/20 border border-white/10 rounded-lg text-white focus:outline-none focus:border-blue-500 transition-colors"
            >
              <option value="deepseek-v4-flash">deepseek-v4-flash (推荐)</option>
              <option value="deepseek-v4-pro">deepseek-v4-pro</option>
            </select>
          </div>

          <div className="flex flex-col space-y-1.5">
            <label className="text-sm font-medium text-white/80">
              Base URL
            </label>
            <input
              type="text"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              className="px-3 py-2 bg-black/20 border border-white/10 rounded-lg text-white focus:outline-none focus:border-blue-500 transition-colors"
              placeholder="https://api.deepseek.com"
            />
          </div>
        </div>

        <div className="flex justify-end space-x-3 pt-2">
          <button
            onClick={onClose}
            className="px-4 py-2 text-sm text-white/70 hover:text-white bg-transparent border border-white/10 hover:bg-white/5 rounded-lg transition-colors"
            disabled={isLoading}
          >
            取消
          </button>
          <button
            onClick={handleSave}
            className="px-4 py-2 text-sm text-white bg-blue-600 hover:bg-blue-500 rounded-lg transition-colors disabled:opacity-50"
            disabled={isLoading}
          >
            {isLoading ? "保存中..." : "保存设置"}
          </button>
        </div>
      </div>
    </div>
  );
};
