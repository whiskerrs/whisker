      import init, * as whisker from "{{base_path}}whisker_app.js";
      const reportDevelopmentError = (error) => {
        const output = document.getElementById("whisker-dev-error");
        output.hidden = false;
        output.textContent = `Whisker Web: ${error}`;
      };
      window.addEventListener("error", (event) => reportDevelopmentError(event.error));
      window.addEventListener("unhandledrejection", (event) => reportDevelopmentError(event.reason));
      try {
        await init();
      } catch (error) {
        const root = document.getElementById("whisker-root");
        root.textContent = `Whisker Web failed to start: ${error}`;
        throw error;
      }

      // __WHISKER_DEVELOPMENT_BOOTSTRAP__
