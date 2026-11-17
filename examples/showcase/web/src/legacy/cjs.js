const { clamp } = require('../utils/math');

module.exports = function limit(value) {
  return clamp(value, 0, 100);
};
