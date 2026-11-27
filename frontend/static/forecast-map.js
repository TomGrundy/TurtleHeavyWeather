const MAPLIBRE_ESM = "https://unpkg.com/maplibre-gl@6.19.0/dist/maplibre-gl.mjs";
const OM_LAYER_ESM = "https://unpkg.com/@openmeteo/weather-map-layer@0.3.2/dist/index.mjs";
const BASEMAP_TILE_URL =
  "https://server.arcgisonline.com/ArcGIS/rest/services/Canvas/World_Dark_Gray_Base/MapServer/tile/{z}/{y}/{x}";

let libsPromise = null;
let protocolRegistered = false;
let map = null;
let marker = null;
let mountSeq = 0;
let pendingTimeIdx = null;

function mapZoomFromLevel(zoom) {
  return zoom === 5 ? 4.3 : 6.5;
}

function omSourceUrl(validTimeIdx) {
  return (
    "om://https://openmeteo.s3.us-west-2.amazonaws.com/data_spatial/ecmwf_ifs/latest.json" +
    `?time_step=valid_times_${validTimeIdx}&variable=precipitation`
  );
}

async function ensureLibs() {
  if (!libsPromise) {
    libsPromise = Promise.all([import(MAPLIBRE_ESM), import(OM_LAYER_ESM)]).then(
      ([maplibregl, omLayer]) => {
        if (!protocolRegistered) {
          maplibregl.addProtocol("om", omLayer.omProtocol);
          protocolRegistered = true;
        }
        return { maplibregl, omLayer };
      }
    );
  }
  return libsPromise;
}

function applyPrecipLayer(timeIdx) {
  if (!map || !map.isStyleLoaded()) {
    pendingTimeIdx = timeIdx;
    return;
  }
  const url = omSourceUrl(timeIdx);
  const existing = map.getSource("om-precip");
  if (existing && typeof existing.setUrl === "function") {
    existing.setUrl(url);
    return;
  }
  if (map.getLayer("om-precip-layer")) {
    map.removeLayer("om-precip-layer");
  }
  if (existing) {
    map.removeSource("om-precip");
  }
  map.addSource("om-precip", {
    type: "raster",
    url,
    maxzoom: 12,
  });
  map.addLayer({
    id: "om-precip-layer",
    type: "raster",
    source: "om-precip",
    paint: {
      "raster-opacity": 0.78,
    },
  });
}

function createPinElement() {
  const el = document.createElement("div");
  el.className = "forecast-map-pin";
  return el;
}

window.thwForecastMap = {
  async mount(container, lat, lon, zoom, validTimeIdx) {
    const seq = ++mountSeq;
    this.destroy(false);
    pendingTimeIdx = validTimeIdx;

    const { maplibregl } = await ensureLibs();
    if (seq !== mountSeq || !container || !container.isConnected) {
      return;
    }

    map = new maplibregl.Map({
      container,
      center: [lon, lat],
      zoom: mapZoomFromLevel(zoom),
      attributionControl: false,
      style: {
        version: 8,
        sources: {
          basemap: {
            type: "raster",
            tiles: [BASEMAP_TILE_URL],
            tileSize: 256,
          },
        },
        layers: [
          {
            id: "basemap",
            type: "raster",
            source: "basemap",
          },
        ],
      },
    });

    marker = new maplibregl.Marker({ element: createPinElement() })
      .setLngLat([lon, lat])
      .addTo(map);

    map.on("load", () => {
      if (seq !== mountSeq || !map) return;
      const idx = pendingTimeIdx !== null ? pendingTimeIdx : validTimeIdx;
      pendingTimeIdx = null;
      applyPrecipLayer(idx);
    });
  },

  setTime(validTimeIdx) {
    applyPrecipLayer(validTimeIdx);
  },

  setView(lat, lon, zoom) {
    if (!map) return;
    map.jumpTo({
      center: [lon, lat],
      zoom: mapZoomFromLevel(zoom),
    });
    if (marker) {
      marker.setLngLat([lon, lat]);
    }
  },

  destroy(bumpSeq = true) {
    if (bumpSeq) {
      mountSeq++;
    }
    pendingTimeIdx = null;
    if (marker) {
      marker.remove();
      marker = null;
    }
    if (map) {
      map.remove();
      map = null;
    }
  },
};
