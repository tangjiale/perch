import React from "react";

interface State {
  error: Error | null;
}

export default class ErrorBoundary extends React.Component<
  { children: React.ReactNode; resetKey?: string },
  State
> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidUpdate(previous: { resetKey?: string }) {
    if (this.state.error && previous.resetKey !== this.props.resetKey) {
      this.setState({ error: null });
    }
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="error-panel" role="alert">
        <h2>页面渲染遇到异常</h2>
        <p>{this.state.error.message || "未知界面错误"}</p>
        <button
          type="button"
          onClick={() => this.setState({ error: null })}
        >
          重试加载当前页面
        </button>
      </div>
    );
  }
}
