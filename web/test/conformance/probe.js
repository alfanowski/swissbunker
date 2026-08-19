// Shared harness for Phase 0 probes.
// Deliberately a classic script with no imports and no dependencies: the whole point of
// this spike is to find out what survives a null origin, so the harness itself must not
// rely on anything we are still trying to prove works.
(function (global) {
  'use strict';

  var state = null;

  function detectContext() {
    return {
      origin: global.location.origin,       // "null" under file://
      protocol: global.location.protocol,
      userAgent: navigator.userAgent,
      platform: navigator.userAgentData ? navigator.userAgentData.platform : navigator.platform,
      isSecureContext: global.isSecureContext,
      crossOriginIsolated: global.crossOriginIsolated,
      hasSharedArrayBuffer: typeof SharedArrayBuffer !== 'undefined',
      hasWebGPU: typeof navigator.gpu !== 'undefined',
      deviceMemoryGB: navigator.deviceMemory || null,
      hardwareConcurrency: navigator.hardwareConcurrency || null
    };
  }

  function render() {
    var el = document.getElementById('probe-output');
    if (!el) { return; }
    el.textContent = JSON.stringify(state, null, 2);
  }

  var Probe = {
    init: function (id, title) {
      state = {
        probe: id,
        title: title,
        // Timestamp is filled in by the operator when filing the result, not here:
        // the clock of a random test machine is not trustworthy metadata.
        context: detectContext(),
        checks: {},
        measurements: {},
        info: {}
      };
      document.title = 'Probe ' + id + ' — ' + title;
      render();
      return state;
    },

    check: async function (name, fn) {
      var result;
      try {
        var value = await fn();
        result = { ok: value !== false && value !== null && value !== undefined, detail: value };
      } catch (err) {
        // A thrown exception IS the finding here, so it is recorded rather than propagated.
        result = { ok: false, detail: String(err && err.message ? err.message : err) };
      }
      state.checks[name] = result;
      render();
      return result;
    },

    measure: async function (name, fn, runs) {
      runs = runs || 20;
      var samples = [];
      for (var i = 0; i < runs; i++) {
        var t0 = performance.now();
        await fn(i);
        samples.push(performance.now() - t0);
      }
      samples.sort(function (a, b) { return a - b; });
      var pick = function (q) {
        return samples[Math.min(samples.length - 1, Math.floor(samples.length * q))];
      };
      var result = {
        runs: runs,
        p50: Math.round(pick(0.50) * 1000) / 1000,
        p95: Math.round(pick(0.95) * 1000) / 1000,
        min: Math.round(samples[0] * 1000) / 1000,
        max: Math.round(samples[samples.length - 1] * 1000) / 1000
      };
      state.measurements[name] = result;
      render();
      return result;
    },

    info: function (key, value) {
      state.info[key] = value;
      render();
    },

    finish: function () {
      var blob = new Blob([JSON.stringify(state, null, 2)], { type: 'application/json' });
      var a = document.createElement('a');
      a.href = URL.createObjectURL(blob);
      a.download = state.probe + '-' + (state.context.protocol === 'file:' ? 'file' : 'http') + '.json';
      a.textContent = 'Download result JSON';
      a.style.cssText = 'display:inline-block;margin:1rem 0;padding:.6rem 1rem;background:#111;color:#fff;text-decoration:none;border-radius:6px';
      document.body.appendChild(a);
      render();
      return state;
    }
  };

  global.Probe = Probe;
}(window));
