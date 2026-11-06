/* The Atlas view of the web pages' Memories list (ADR-0045 d25-d27, d30):
   the OML Live Atlas of docs/atlas, "a quiet instrument room over a living
   ledger", drawn over real data instead of a mocked desk.

   Each workarea the viewer may read is a region on the floor, sized by its
   readable rows and nested inside its parent; each memory of this page is a
   point inside its workarea's region, its state a colour and a form. The
   data is the JSON route named by #atlas[data-src], with this page's own
   query, so the drawing never shows a wider set than the list. The script
   writes only through textContent and the CSSOM, never through markup, and
   nothing here is inline: the page's policy admits this file and three.js,
   by path, and nothing else. */
(function () {
  'use strict';
  var root = document.getElementById('atlas');
  var list = document.getElementById('atlas-list');
  var regionList = document.getElementById('atlas-regions');
  var crumbs = document.getElementById('atlas-crumbs');
  var status = document.getElementById('atlas-status');
  var panel = document.getElementById('atlas-panel');
  if (!root || !list) { return; }
  var html = document.documentElement;
  var REDUCED = window.matchMedia && matchMedia('(prefers-reduced-motion: reduce)').matches;
  var base = '/web';

  function say(text) { if (status) { status.textContent = text; } }
  function el(tag, cls, text) {
    var e = document.createElement(tag);
    if (cls) { e.className = cls; }
    if (text !== undefined && text !== null) { e.textContent = String(text); }
    return e;
  }
  function clear(node) { while (node.firstChild) { node.removeChild(node.firstChild); } }

  /* One law for colour, as in docs/atlas: warmth belongs to memory (amber
     held, bronze lowered), cool to structure; each state is also a form. */
  var STATE = {
    held: { color: 0xe8b45a, form: 'ring' },
    promoted: { color: 0x46d684, form: 'core' },
    superseded: { color: 0x64748b, form: 'dia' },
    rejected: { color: 0xf0716e, form: 'sq' },
    demoted: { color: 0xb08968, form: 'dia' }
  };
  function stateOf(item) { return STATE[item.state] || STATE.superseded; }

  /* A link to this view with `workarea` set (or cleared) and the cursor
     dropped: the list and the Atlas filter alike (d30). */
  function filtered(workareaId, atlas) {
    var params = new URLSearchParams(location.search);
    params.delete('cursor');
    params.delete('memory');
    if (workareaId) { params.set('workarea', workareaId); } else { params.delete('workarea'); }
    if (atlas) { params.set('display', 'atlas'); } else { params.delete('display'); }
    params.set('view', 'memories');
    return base + '?' + params.toString();
  }

  function countLine(c) {
    var parts = [c.approved + ' approved', c.held + ' held'];
    if (c.in_progress) { parts.push(c.in_progress + ' in progress'); }
    if (c.other) { parts.push(c.other + ' other'); }
    return parts.join(' · ');
  }

  var selected = null;
  function openPanel() { panel.hidden = false; }
  function panelHead() {
    clear(panel);
    var close = el('button', 'x', '✕');
    close.type = 'button';
    close.setAttribute('aria-label', 'close');
    close.addEventListener('click', function () { panel.hidden = true; selected = null; });
    panel.appendChild(close);
  }

  /* A memory's panel: the fields the card shows, nothing more. */
  function showItem(item) {
    selected = { item: item };
    if (!panel) { return; }
    panelHead();
    var head = el('p', 'label');
    head.appendChild(el('span', 'tag ' + (item.state === 'held' ? 'held' : item.state === 'promoted' ? 'ok' : 'dim'), item.state));
    head.appendChild(document.createTextNode(' · ' + item.workarea + ' · version ' + item.version));
    panel.appendChild(head);
    if (item.title) { panel.appendChild(el('p', 'title', item.title)); }
    panel.appendChild(el('p', 'text', item.text));
    if (item.state === 'held') {
      panel.appendChild(el('p', 'held', 'Held until someone else reviews it.'));
    }
    panel.appendChild(el('p', 'dim mono', item.claim_key + ' · from ' + (item.author || 'the shared area')));
    if (item.anchor) { panel.appendChild(el('p', 'dim mono', 'source: ' + item.anchor)); }
    if (item.format) { panel.appendChild(el('p', 'dim mono', 'format: ' + item.format)); }
    if (item.valid) { panel.appendChild(el('p', 'dim mono', 'valid ' + item.valid)); }
    var open = el('a', null, 'Open this memory');
    open.href = base + '?view=memories&memory=' + encodeURIComponent(item.artifact_id);
    panel.appendChild(open);
    openPanel();
  }

  /* A workarea's panel: its name, counts and badges, the filter that zooms
     the Atlas and the list to it, and its dropdown of the acts the page's
     keys may take there now, which asks to confirm first (d28). */
  function showRegion(r) {
    selected = { region: r };
    if (!panel) { return; }
    panelHead();
    var head = el('p', 'label', 'workarea · ' + r.status);
    panel.appendChild(head);
    panel.appendChild(el('p', 'title', r.path));
    panel.appendChild(el('p', null, countLine(r.counts)));
    var badges = el('p');
    if (r.in_progress) { badges.appendChild(el('span', 'tag held', 'in progress')); }
    if (r.ontology) { badges.appendChild(el('span', 'tag ok', 'ontology')); }
    if (badges.firstChild) { panel.appendChild(badges); }
    var zoom = el('a', null, 'Show only this workarea');
    zoom.href = filtered(r.id, true);
    panel.appendChild(zoom);
    panel.appendChild(document.createTextNode(' · '));
    var asList = el('a', null, 'as a list');
    asList.href = filtered(r.id, false);
    panel.appendChild(asList);
    if (r.offers.length || r.commit_withheld !== null) {
      var form = el('form', 'row act');
      form.method = 'get';
      form.action = base;
      [['view', 'workarea'], ['workarea', r.id]].forEach(function (kv) {
        var h = el('input');
        h.type = 'hidden'; h.name = kv[0]; h.value = kv[1];
        form.appendChild(h);
      });
      var label = el('label', null, 'act');
      var select = el('select');
      select.name = 'op';
      r.offers.forEach(function (o) {
        var opt = el('option', null, o.text);
        opt.value = o.value;
        select.appendChild(opt);
      });
      if (r.commit_withheld !== null) {
        var w = el('option', null, 'commit — ' + r.commit_withheld + (r.commit_withheld === 1 ? ' memory' : ' memories') + ' still held');
        w.disabled = true;
        if (!r.offers.length) { w.selected = true; }
        select.appendChild(w);
      }
      label.appendChild(select);
      form.appendChild(label);
      if (r.offers.length) {
        var apply = el('button', null, 'Apply');
        apply.type = 'submit';
        form.appendChild(apply);
      }
      panel.appendChild(form);
    }
    openPanel();
  }

  /* The text alternative: the workareas, each with its counts, and this
     page's memories, each opening the same panel the drawing does. */
  function buildLists(items, regions, depthOf) {
    clear(list);
    items.forEach(function (item) {
      var li = el('li');
      var b = el('button', 'linkish', item.state + ' · ' + item.workarea + ' · ' + (item.title || item.text).slice(0, 120));
      b.type = 'button';
      b.addEventListener('click', function () { showItem(item); });
      li.appendChild(b);
      list.appendChild(li);
    });
    if (!items.length) { list.appendChild(el('li', 'dim', 'No memory on this page.')); }
    if (!regionList) { return; }
    clear(regionList);
    regions.forEach(function (r) {
      var li = el('li');
      li.style.marginLeft = (depthOf(r) * 1.2) + 'em';
      var b = el('button', 'linkish', r.path);
      b.type = 'button';
      b.addEventListener('click', function () { showRegion(r); });
      li.appendChild(b);
      li.appendChild(el('span', 'dim', ' — ' + r.counts.total + ' readable: ' + countLine(r.counts) + (r.in_progress ? ' · in progress' : '')));
      regionList.appendChild(li);
    });
  }

  /* The breadcrumb: back out to every workarea, and one level up. */
  function buildCrumbs(regions, top) {
    if (!crumbs) { return; }
    clear(crumbs);
    var params = new URLSearchParams(location.search);
    var all = el('a', null, 'All workareas');
    all.href = filtered(null, true);
    crumbs.appendChild(all);
    if (!params.get('workarea') || !top) { return; }
    if (top.parent) {
      crumbs.appendChild(document.createTextNode(' › '));
      var up = el('a', null, 'up one level');
      up.href = filtered(top.parent, true);
      crumbs.appendChild(up);
    }
    crumbs.appendChild(document.createTextNode(' › '));
    crumbs.appendChild(el('span', null, top.path));
  }

  function nextLink(cursor) {
    var pager = document.getElementById('atlas-next');
    if (!pager || !cursor) { return; }
    var params = new URLSearchParams(location.search);
    params.set('cursor', cursor);
    var a = el('a', null, 'Next page');
    a.rel = 'next';
    a.href = location.pathname + '?' + params.toString();
    pager.appendChild(a);
  }

  /* Circle packing that stays calm: a region's own rows fill its centre,
     its children sit on a ring around it, spaced by their size. */
  function layout(regions) {
    var byId = {};
    regions.forEach(function (r) { r.children = []; byId[r.id] = r; });
    var tops = [];
    regions.forEach(function (r) {
      if (r.parent && byId[r.parent]) { byId[r.parent].children.push(r); } else { tops.push(r); }
    });
    function size(r) {
      r.core = 1.1 + Math.sqrt(r.counts.total) * 0.55;
      r.children.forEach(size);
      r.children.sort(function (a, b) { return b.radius - a.radius; });
      if (!r.children.length) { r.radius = r.core; return; }
      var gap = 0.5, max = 0, around = 0;
      r.children.forEach(function (c) { max = Math.max(max, c.radius); around += 2 * c.radius + gap; });
      r.ring = Math.max(r.core + max + gap, around / (2 * Math.PI));
      r.radius = r.ring + max + gap;
    }
    function place(r, x, z, depth) {
      r.x = x; r.z = z; r.depth = depth;
      var angle = -Math.PI / 2;
      var total = 0;
      r.children.forEach(function (c) { total += 2 * c.radius + 0.5; });
      var spare = r.children.length ? (2 * Math.PI * r.ring - total) / r.children.length : 0;
      r.children.forEach(function (c) {
        var step = (c.radius + 0.25 + spare / 2) / r.ring;
        angle += step;
        place(c, x + Math.cos(angle) * r.ring, z + Math.sin(angle) * r.ring, depth + 1);
        angle += step;
      });
    }
    var world = { children: tops, counts: { total: 0 }, id: null };
    tops.forEach(size);
    if (tops.length === 1) {
      place(tops[0], 0, 0, 0);
      world.radius = tops[0].radius;
    } else {
      size(world);
      place(world, 0, 0, -1);
    }
    return { tops: tops, radius: world.radius || 1, byId: byId };
  }

  function draw(items, regions) {
    var tree = layout(regions);
    var depthOf = function (r) { return Math.max(0, r.depth || 0); };
    buildLists(items, regions, depthOf);
    var top = tree.tops.length === 1 ? tree.tops[0] : null;
    buildCrumbs(regions, top);
    if (!window.THREE) { say('The drawing library did not load; the page is listed below.'); html.setAttribute('data-atlas', 'failed'); return; }
    var THREE = window.THREE;
    var canvas = document.getElementById('atlas-canvas');
    var renderer;
    try {
      renderer = new THREE.WebGLRenderer({ canvas: canvas, antialias: true, alpha: true });
    } catch (e) {
      say('This browser draws no WebGL; the workareas and the page are listed below.');
      html.setAttribute('data-atlas', 'no-webgl');
      return;
    }
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.setClearColor(0x04060d, 1);
    var scene = new THREE.Scene();
    scene.fog = new THREE.FogExp2(0x04060d, 0.006);
    var camera = new THREE.PerspectiveCamera(46, 2, 0.1, 600);
    var scale = 26 / tree.radius;
    var cam = { theta: 0.35, phi: 0.82, radius: 52, min: 10, max: 110, target: new THREE.Vector3() };
    function applyCam() {
      cam.phi = Math.max(0.12, Math.min(1.45, cam.phi));
      cam.radius = Math.max(cam.min, Math.min(cam.max, cam.radius));
      camera.position.set(
        cam.target.x + cam.radius * Math.sin(cam.phi) * Math.sin(cam.theta),
        cam.target.y + cam.radius * Math.cos(cam.phi),
        cam.target.z + cam.radius * Math.sin(cam.phi) * Math.cos(cam.theta));
      camera.lookAt(cam.target);
    }

    function texture(kind) {
      var c = document.createElement('canvas');
      c.width = c.height = 128;
      var g = c.getContext('2d');
      g.translate(64, 64);
      g.fillStyle = '#fff';
      g.strokeStyle = '#fff';
      if (kind === 'core' || kind === 'soft') {
        var gr = g.createRadialGradient(0, 0, 0, 0, 0, 60);
        gr.addColorStop(0, 'rgba(255,255,255,1)');
        gr.addColorStop(kind === 'core' ? 0.55 : 0.3, kind === 'core' ? 'rgba(255,255,255,.95)' : 'rgba(255,255,255,.4)');
        gr.addColorStop(1, 'rgba(255,255,255,0)');
        g.fillStyle = gr;
        g.fillRect(-64, -64, 128, 128);
      } else if (kind === 'ring') {
        g.lineWidth = 13; g.beginPath(); g.arc(0, 0, 37, 0, Math.PI * 2); g.stroke();
        g.globalAlpha = 0.25; g.lineWidth = 22; g.beginPath(); g.arc(0, 0, 37, 0, Math.PI * 2); g.stroke();
      } else if (kind === 'dia') {
        g.rotate(Math.PI / 4); g.fillRect(-31, -31, 62, 62);
        g.globalAlpha = 0.22; g.fillRect(-42, -42, 84, 84);
      } else {
        g.fillRect(-29, -29, 58, 58); g.globalAlpha = 0.22; g.fillRect(-40, -40, 80, 80);
      }
      return new THREE.CanvasTexture(c);
    }
    var TEX = { core: texture('core'), ring: texture('ring'), dia: texture('dia'), sq: texture('sq'), soft: texture('soft') };
    function glow(color, size, opacity, map) {
      var s = new THREE.Sprite(new THREE.SpriteMaterial({ map: map || TEX.core, color: color, transparent: true, opacity: opacity, blending: THREE.AdditiveBlending, depthWrite: false }));
      s.scale.setScalar(size);
      return s;
    }

    /* the room: a faint starfield and a quiet instrument floor */
    (function () {
      var n = 360, p = new Float32Array(n * 3);
      for (var i = 0; i < n; i++) { p.set([(Math.random() - 0.5) * 260, (Math.random() - 0.5) * 140, (Math.random() - 0.5) * 260], i * 3); }
      var g = new THREE.BufferGeometry();
      g.setAttribute('position', new THREE.BufferAttribute(p, 3));
      var m = new THREE.PointsMaterial({ size: 0.3, map: TEX.soft, color: 0x55639a, transparent: true, opacity: 0.6, depthWrite: false });
      m.fog = false;
      scene.add(new THREE.Points(g, m));
    })();
    var system = new THREE.Group();
    scene.add(system);
    var grid = new THREE.PolarGridHelper(34, 8, 4, 64, 0x1e2745, 0x11182e);
    grid.position.y = -0.6;
    grid.material.transparent = true;
    grid.material.opacity = 0.3;
    system.add(grid);

    /* one region a workarea: a rim, a faint floor, a terrace per depth */
    var labels = document.getElementById('atlas-labels');
    var regionHits = [];
    var pointHits = [];
    regions.forEach(function (r) {
      var y = depthOf(r) * 0.35;
      var radius = r.radius * scale;
      var cx = r.x * scale, cz = r.z * scale;
      var rim = r.in_progress ? 0xe8b45a : 0x9fb0d8;
      var pts = [];
      for (var j = 0; j <= 96; j++) { var a = j / 96 * Math.PI * 2; pts.push(new THREE.Vector3(cx + Math.cos(a) * radius, y, cz + Math.sin(a) * radius)); }
      var line = new THREE.Line(new THREE.BufferGeometry().setFromPoints(pts),
        r.in_progress
          ? new THREE.LineDashedMaterial({ color: rim, dashSize: 0.6, gapSize: 0.5, transparent: true, opacity: 0.7 })
          : new THREE.LineBasicMaterial({ color: rim, transparent: true, opacity: 0.32 }));
      if (r.in_progress) { line.computeLineDistances(); }
      system.add(line);
      var floor = new THREE.Mesh(new THREE.CircleGeometry(radius, 64),
        new THREE.MeshBasicMaterial({ color: r.in_progress ? 0x2a2210 : 0x0b1328, transparent: true, opacity: 0.32, depthWrite: false, side: THREE.DoubleSide }));
      floor.rotation.x = -Math.PI / 2;
      floor.position.set(cx, y - 0.01, cz);
      floor.userData.region = r;
      floor.userData.depth = depthOf(r);
      system.add(floor);
      regionHits.push(floor);
      r.anchor = new THREE.Vector3(cx, y, cz - radius);
      r.label = el('div', 'lbl region');
      r.label.appendChild(el('span', null, r.name));
      if (r.in_progress) { r.label.appendChild(el('span', 'badge', 'in progress')); }
      if (r.ontology) { r.label.appendChild(el('span', 'badge', 'ontology')); }
      r.label.appendChild(el('small', null, countLine(r.counts)));
      labels.appendChild(r.label);
      /* this page's memories on this workarea, in its centre */
      var own = items.filter(function (it) { return it.workarea === r.path; });
      var inner = r.core * scale * 0.82;
      own.forEach(function (item, k) {
        var st = stateOf(item);
        var t = (k + 0.5) / Math.max(1, own.length);
        var dist = inner * Math.sqrt(t);
        var ang = k * 2.399;
        var s = glow(st.color, st.form === 'ring' ? 0.95 : 0.8, 0.95, TEX[st.form]);
        s.position.set(cx + Math.cos(ang) * dist, y + 0.35, cz + Math.sin(ang) * dist);
        s.userData.item = item;
        system.add(s);
        pointHits.push(s);
      });
    });
    var marker = glow(0xffffff, 1.5, 0.9, TEX.ring);
    marker.visible = false;
    scene.add(marker);

    var ray = new THREE.Raycaster();
    var mouse = new THREE.Vector2();
    var drag = null, moved = false;
    canvas.addEventListener('pointerdown', function (e) { drag = [e.clientX, e.clientY]; moved = false; canvas.setPointerCapture(e.pointerId); });
    canvas.addEventListener('pointermove', function (e) {
      if (!drag) { return; }
      var dx = e.clientX - drag[0], dy = e.clientY - drag[1];
      if (Math.abs(dx) + Math.abs(dy) > 4) { moved = true; }
      cam.theta -= dx * 0.006; cam.phi -= dy * 0.006; drag = [e.clientX, e.clientY]; applyCam();
    });
    canvas.addEventListener('pointerup', function (e) {
      drag = null;
      if (moved) { return; }
      var b = canvas.getBoundingClientRect();
      mouse.set(((e.clientX - b.left) / b.width) * 2 - 1, -((e.clientY - b.top) / b.height) * 2 + 1);
      ray.setFromCamera(mouse, camera);
      var point = ray.intersectObjects(pointHits, false);
      if (point.length) { showItem(point[0].object.userData.item); return; }
      var hits = ray.intersectObjects(regionHits, false);
      if (hits.length) {
        hits.sort(function (a, b2) { return b2.object.userData.depth - a.object.userData.depth; });
        showRegion(hits[0].object.userData.region);
      }
    });
    canvas.addEventListener('wheel', function (e) { e.preventDefault(); cam.radius *= 1 + Math.sign(e.deltaY) * 0.08; applyCam(); }, { passive: false });

    var v = new THREE.Vector3();
    function placeLabel(r) {
      v.copy(r.anchor).project(camera);
      var b = canvas.getBoundingClientRect();
      var x = (v.x * 0.5 + 0.5) * b.width, y = (-v.y * 0.5 + 0.5) * b.height;
      r.label.style.transform = 'translate3d(' + x.toFixed(1) + 'px,' + y.toFixed(1) + 'px,0) translate(-50%,-110%)';
      r.label.style.opacity = v.z < 1 ? '0.94' : '0';
    }
    var last = performance.now();
    function frame(now) {
      var dt = Math.min(0.05, (now - last) / 1000);
      last = now;
      var w = canvas.clientWidth, h = canvas.clientHeight;
      if (canvas.width !== Math.floor(w * renderer.getPixelRatio()) || canvas.height !== Math.floor(h * renderer.getPixelRatio())) {
        renderer.setSize(w, h, false);
        camera.aspect = w / Math.max(1, h);
        camera.updateProjectionMatrix();
      }
      /* a slow drift of the whole room, never of the data within it */
      if (!REDUCED && !drag) { cam.theta += dt * 0.02; applyCam(); }
      if (selected && selected.item) {
        var hit = pointHits.filter(function (s) { return s.userData.item === selected.item; })[0];
        if (hit) { marker.visible = true; hit.getWorldPosition(marker.position); }
      } else { marker.visible = false; }
      renderer.render(scene, camera);
      regions.forEach(placeLabel);
      requestAnimationFrame(frame);
    }
    applyCam();
    requestAnimationFrame(frame);
    say(regions.length + (regions.length === 1 ? ' workarea' : ' workareas') + ', ' + items.length + (items.length === 1 ? ' memory' : ' memories') + ' on this page. Drag to turn, scroll to zoom, select to read.');
    html.setAttribute('data-atlas', 'drawn');
  }

  var src = root.getAttribute('data-src') + location.search;
  fetch(src, { credentials: 'same-origin', headers: { Accept: 'application/json' } })
    .then(function (r) {
      if (!r.ok) { throw new Error('the page answered ' + r.status); }
      return r.json();
    })
    .then(function (page) {
      base = page.base || base;
      var items = page.items || [];
      var regions = (page.workareas && page.workareas.items) || [];
      if (page.workareas && page.workareas.partial) {
        say('Counts stop at the first rows read; each count is at least the number shown.');
      }
      nextLink(page.next_cursor);
      html.setAttribute('data-atlas-items', String(items.length));
      html.setAttribute('data-atlas-regions', String(regions.length));
      draw(items, regions);
    })
    .catch(function (e) {
      say('The page could not be read (' + e.message + '); the list view shows it.');
      html.setAttribute('data-atlas', 'failed');
    });
})();
